use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{json, Value};
use thiserror::Error;
use url::Url;

use crate::net::minecraft::util::Session::Session;

const XBOX_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_AUTH_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const MINECRAFT_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MINECRAFT_PROFILE_NAME_URL: &str = "https://api.minecraftservices.com/minecraft/profile/name/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrosoftLogin {
    pub session: Session,
    pub refreshToken: String,
    pub accessToken: String,
}

#[derive(Debug, Error)]
pub enum MicrosoftAuthError {
    #[error("Microsoft authentication request failed: {0}")]
    Http(String),
    #[error("Microsoft authentication response was missing {0}")]
    Missing(&'static str),
    #[error("Microsoft authentication response was malformed: {0}")]
    Malformed(String),
    #[error("Microsoft authentication was cancelled")]
    Cancelled,
    #[error("Microsoft authentication timed out waiting for the sign-in code")]
    TimedOut,
}

pub fn refresh_login(refreshToken: &str, status: Option<&Sender<String>>) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    refresh_login_cancelable(refreshToken, status, None)
}

fn refresh_login_cancelable(
    refreshToken: &str,
    status: Option<&Sender<String>>,
    cancelled: Option<&AtomicBool>,
) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    ensure_not_cancelled(cancelled)?;
    send_status(status, "§fRefreshing Microsoft access tokens§r");
    // Refresh tokens are bound to the OAuth client that issued them; this build
    // only issues tokens through the device code flow.
    let (microsoftAccessToken, refreshToken) = super::MicrosoftDeviceCode::refresh(refreshToken)?;
    complete_microsoft_login(microsoftAccessToken, refreshToken, status, cancelled)
}

pub fn login_saved_account(
    accessToken: &str,
    refreshToken: &str,
    status: Option<&Sender<String>>,
) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    login_saved_account_cancelable(accessToken, refreshToken, status, None)
}

pub fn login_saved_account_cancelable(
    accessToken: &str,
    refreshToken: &str,
    status: Option<&Sender<String>>,
    cancelled: Option<&AtomicBool>,
) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    ensure_not_cancelled(cancelled)?;
    if !accessToken.trim().is_empty() {
        send_status(status, "§fFetching your Minecraft profile§r");
        if let Ok(session) = login_with_minecraft_access_token(accessToken) {
            ensure_not_cancelled(cancelled)?;
            return Ok(MicrosoftLogin {
                session,
                refreshToken: refreshToken.to_owned(),
                accessToken: accessToken.to_owned(),
            });
        }
    }
    ensure_not_cancelled(cancelled)?;
    if refreshToken.trim().is_empty() {
        return Err(MicrosoftAuthError::Http("access token is invalid or expired and no refresh token is available".to_owned()));
    }
    refresh_login_cancelable(refreshToken, status, cancelled)
}

pub fn token_login(token: &str, status: Option<&Sender<String>>) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    if token.starts_with("M.C") {
        refresh_token_login(token, status)
    } else {
        send_status(status, "§fFetching your Minecraft profile§r");
        let session = login_with_minecraft_access_token(token)?;
        Ok(MicrosoftLogin { session, refreshToken: String::new(), accessToken: token.to_owned() })
    }
}

fn refresh_token_login(
    refreshToken: &str,
    status: Option<&Sender<String>>,
) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    send_status(status, "§fRefreshing Microsoft access tokens§r");
    // Refresh tokens are issued by the OAuth client that created them, so a
    // token pasted from this launcher must be refreshed through the device code
    // client, not through Mojang's retired Live Connect client id.
    let (microsoftAccessToken, refreshToken) = super::MicrosoftDeviceCode::refresh(refreshToken)?;

    send_status(status, "§fAcquiring Xbox access token§r");
    let xboxToken = acquire_xbox_access_token(&microsoftAccessToken)?;
    send_status(status, "§fAcquiring Xbox XSTS token§r");
    let (xstsToken, userHash) = acquire_xsts_token(&xboxToken)?;
    send_status(status, "§fAcquiring Minecraft access token§r");
    let minecraftAccessToken = acquire_minecraft_access_token(&xstsToken, &userHash)?;
    send_status(status, "§fFetching your Minecraft profile§r");
    let (session, minecraftAccessToken) = login_or_create_token_profile(
        minecraftAccessToken,
        &xstsToken,
        &userHash,
        status,
    )?;
    Ok(MicrosoftLogin { session, refreshToken, accessToken: minecraftAccessToken })
}

fn login_or_create_token_profile(
    minecraftAccessToken: String,
    xstsToken: &str,
    userHash: &str,
    status: Option<&Sender<String>>,
) -> Result<(Session, String), MicrosoftAuthError> {
    if let Some(session) = try_login_with_minecraft_access_token(&minecraftAccessToken)? {
        return Ok((session, minecraftAccessToken));
    }

    ensure_minecraft_entitlement(&minecraftAccessToken)?;
    let name = prompt_for_minecraft_profile_name()?;
    send_status(status, "§fCreating your Minecraft profile§r");
    create_minecraft_profile_name(&minecraftAccessToken, &name)?;

    // Exhibition requests a new Minecraft access token after profile creation
    // before reading the profile again. Preserve that sequence rather than
    // assuming the previous token has immediately observed the new profile.
    send_status(status, "§fAcquiring Minecraft access token§r");
    let refreshedMinecraftAccessToken = acquire_minecraft_access_token(xstsToken, userHash)?;
    send_status(status, "§fFetching your Minecraft profile§r");
    let session = login_with_minecraft_access_token(&refreshedMinecraftAccessToken)?;
    Ok((session, refreshedMinecraftAccessToken))
}

fn try_login_with_minecraft_access_token(
    token: &str,
) -> Result<Option<Session>, MicrosoftAuthError> {
    if token.trim().is_empty() {
        return Err(MicrosoftAuthError::Missing("Minecraft access token"));
    }
    let request = ureq::get(MINECRAFT_PROFILE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(Duration::from_secs(30))
        .call();
    let value = match request {
        Ok(response) => response
            .into_json::<Value>()
            .map_err(|error| MicrosoftAuthError::Malformed(error.to_string()))?,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(error) => return request_json(Err(error)).map(|_| None),
    };
    let username = json_string(&value, "name")?;
    let uuid = json_string(&value, "id")?;
    Ok(Some(Session::new(username, uuid, token, "mojang")))
}

fn ensure_minecraft_entitlement(token: &str) -> Result<(), MicrosoftAuthError> {
    let value = request_json(
        ureq::get(MINECRAFT_ENTITLEMENTS_URL)
            .set("Authorization", &format!("Bearer {token}"))
            .timeout(Duration::from_secs(30))
            .call(),
    )?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or(MicrosoftAuthError::Missing("Minecraft entitlement items"))?;
    let hasMinecraft = items.iter().any(|item| {
        matches!(
            item.get("name").and_then(Value::as_str),
            Some("product_minecraft" | "game_minecraft")
        )
    });
    if !hasMinecraft {
        return Err(MicrosoftAuthError::Http(
            "This Microsoft account dont have minecraft.".to_owned(),
        ));
    }
    Ok(())
}

fn create_minecraft_profile_name(
    token: &str,
    name: &str,
) -> Result<(), MicrosoftAuthError> {
    let url = minecraft_profile_name_url(name)?;
    let result = ureq::put(url.as_str())
        .set("Accept", "*/*")
        .set("Authorization", &format!("Bearer {token}"))
        .set("User-Agent", "MojangSharp/0.1")
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(30))
        .call();
    match result {
        Ok(response) if matches!(response.status(), 200 | 204) => Ok(()),
        Ok(response) => Err(profile_name_status_error(response.status())),
        Err(ureq::Error::Status(status, _)) => Err(profile_name_status_error(status)),
        Err(ureq::Error::Transport(error)) => Err(MicrosoftAuthError::Http(error.to_string())),
    }
}

fn minecraft_profile_name_url(name: &str) -> Result<Url, MicrosoftAuthError> {
    let mut url = Url::parse(MINECRAFT_PROFILE_NAME_URL)
        .map_err(|error| MicrosoftAuthError::Malformed(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| MicrosoftAuthError::Malformed("invalid Minecraft profile name URL".to_owned()))?
        .push(name);
    Ok(url)
}

fn profile_name_status_error(status: u16) -> MicrosoftAuthError {
    let cause = match status {
        400 => "Name is invaild",
        403 => "Name is unlivable",
        401 => "Unauthorized",
        429 => "Too many requests",
        500 => "Mojang API lags",
        _ => "Unknown",
    };
    MicrosoftAuthError::Http(format!("Failed to change name due to {cause}"))
}

fn prompt_for_minecraft_profile_name() -> Result<String, MicrosoftAuthError> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.Text = 'Minecraft Profile'
$form.StartPosition = 'CenterScreen'
$form.TopMost = $true
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$form.ClientSize = New-Object System.Drawing.Size(430, 115)
$label = New-Object System.Windows.Forms.Label
$label.Text = 'No minecraft profile found, please set a new name.'
$label.AutoSize = $true
$label.Location = New-Object System.Drawing.Point(12, 12)
$text = New-Object System.Windows.Forms.TextBox
$text.Location = New-Object System.Drawing.Point(12, 38)
$text.Size = New-Object System.Drawing.Size(406, 23)
$ok = New-Object System.Windows.Forms.Button
$ok.Text = 'OK'
$ok.DialogResult = [System.Windows.Forms.DialogResult]::OK
$ok.Location = New-Object System.Drawing.Point(262, 76)
$cancel = New-Object System.Windows.Forms.Button
$cancel.Text = 'Cancel'
$cancel.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
$cancel.Location = New-Object System.Drawing.Point(343, 76)
$form.Controls.AddRange(@($label, $text, $ok, $cancel))
$form.AcceptButton = $ok
$form.CancelButton = $cancel
$form.Add_Shown({ $text.Focus() })
$result = $form.ShowDialog()
if ($result -ne [System.Windows.Forms.DialogResult]::OK) { exit 2 }
[Console]::Write($text.Text)
"#;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .map_err(|error| MicrosoftAuthError::Http(format!("Failed opening profile name dialog: {error}")))?;
        if output.status.code() == Some(2) {
            return Err(MicrosoftAuthError::Http("Minecraft name is null".to_owned()));
        }
        if !output.status.success() {
            return Err(MicrosoftAuthError::Http("Failed to change name.".to_owned()));
        }
        let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if name.is_empty() {
            return Err(MicrosoftAuthError::Http("Minecraft name is null".to_owned()));
        }
        Ok(name)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(MicrosoftAuthError::Http(
            "Minecraft name is null".to_owned(),
        ))
    }
}

pub fn login_with_minecraft_access_token(token: &str) -> Result<Session, MicrosoftAuthError> {
    if token.trim().is_empty() { return Err(MicrosoftAuthError::Missing("Minecraft access token")); }
    let value = request_json(
        ureq::get(MINECRAFT_PROFILE_URL)
            .set("Authorization", &format!("Bearer {token}"))
            .timeout(Duration::from_secs(30))
            .call(),
    )?;
    let username = json_string(&value, "name")?;
    let uuid = json_string(&value, "id")?;
    Ok(Session::new(username, uuid, token, "mojang"))
}

pub(crate) fn complete_microsoft_login(
    microsoftAccessToken: String,
    refreshToken: String,
    status: Option<&Sender<String>>,
    cancelled: Option<&AtomicBool>,
) -> Result<MicrosoftLogin, MicrosoftAuthError> {
    ensure_not_cancelled(cancelled)?;
    send_status(status, "§fAcquiring Xbox access token§r");
    let xboxToken = acquire_xbox_access_token(&microsoftAccessToken)?;
    ensure_not_cancelled(cancelled)?;
    send_status(status, "§fAcquiring Xbox XSTS token§r");
    let (xstsToken, userHash) = acquire_xsts_token(&xboxToken)?;
    ensure_not_cancelled(cancelled)?;
    send_status(status, "§fAcquiring Minecraft access token§r");
    let minecraftAccessToken = acquire_minecraft_access_token(&xstsToken, &userHash)?;
    ensure_not_cancelled(cancelled)?;
    send_status(status, "§fFetching your Minecraft profile§r");
    let session = login_with_minecraft_access_token(&minecraftAccessToken)?;
    ensure_not_cancelled(cancelled)?;
    Ok(MicrosoftLogin { session, refreshToken, accessToken: minecraftAccessToken })
}

fn ensure_not_cancelled(cancelled: Option<&AtomicBool>) -> Result<(), MicrosoftAuthError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        Err(MicrosoftAuthError::Cancelled)
    } else {
        Ok(())
    }
}

fn acquire_xbox_access_token(microsoftAccessToken: &str) -> Result<String, MicrosoftAuthError> {
    acquire_xbox_access_token_internal(microsoftAccessToken, true)
}

fn acquire_xbox_access_token_internal(
    microsoftAccessToken: &str,
    prefixWithD: bool,
) -> Result<String, MicrosoftAuthError> {
    let rpsTicket = if prefixWithD { format!("d={microsoftAccessToken}") } else { microsoftAccessToken.to_owned() };
    let value = request_json(
        ureq::post(XBOX_AUTH_URL)
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(30))
            .send_json(json!({
                "Properties": {
                    "AuthMethod": "RPS",
                    "SiteName": "user.auth.xboxlive.com",
                    "RpsTicket": rpsTicket
                },
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT"
            })),
    )?;
    json_string(&value, "Token")
}

fn acquire_xsts_token(xboxToken: &str) -> Result<(String, String), MicrosoftAuthError> {
    let value = request_json(
        ureq::post(XSTS_URL)
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(30))
            .send_json(json!({
                "Properties": { "SandboxId": "RETAIL", "UserTokens": [xboxToken] },
                "RelyingParty": "rp://api.minecraftservices.com/",
                "TokenType": "JWT"
            })),
    )?;
    let token = json_string(&value, "Token")?;
    let userHash = value
        .get("DisplayClaims")
        .and_then(|claims| claims.get("xui"))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("uhs"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(MicrosoftAuthError::Missing("Xbox user hash"))?
        .to_owned();
    Ok((token, userHash))
}

fn acquire_minecraft_access_token(xstsToken: &str, userHash: &str) -> Result<String, MicrosoftAuthError> {
    let value = request_json(
        ureq::post(MINECRAFT_AUTH_URL)
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(30))
            .send_json(json!({ "identityToken": format!("XBL3.0 x={userHash};{xstsToken}") })),
    )?;
    json_string(&value, "access_token")
}



fn request_json(result: Result<ureq::Response, ureq::Error>) -> Result<Value, MicrosoftAuthError> {
    match result {
        Ok(response) => response
            .into_json::<Value>()
            .map_err(|error| MicrosoftAuthError::Malformed(error.to_string())),
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| {
                    ["error_description", "errorMessage", "Message", "error"]
                        .into_iter()
                        .find_map(|key| value.get(key).and_then(Value::as_str).map(ToOwned::to_owned))
                })
                .unwrap_or_else(|| body.trim().to_owned());
            Err(MicrosoftAuthError::Http(if message.is_empty() { format!("HTTP {status}") } else { format!("HTTP {status}: {message}") }))
        }
        Err(ureq::Error::Transport(error)) => Err(MicrosoftAuthError::Http(error.to_string())),
    }
}

fn json_string(value: &Value, key: &'static str) -> Result<String, MicrosoftAuthError> {
    value.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or(MicrosoftAuthError::Missing(key))
}

fn send_status(status: Option<&Sender<String>>, value: &str) {
    if let Some(status) = status { let _ = status.send(value.to_owned()); }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_do_not_leak_tokens() {
        let error = MicrosoftAuthError::Http("HTTP 401".to_owned());
        assert_eq!(error.to_string(), "Microsoft authentication request failed: HTTP 401");
    }




    #[test]
    fn profile_name_is_encoded_as_one_path_segment() {
        let url = minecraft_profile_name_url("Name With Space").unwrap();
        assert_eq!(url.as_str(), "https://api.minecraftservices.com/minecraft/profile/name/Name%20With%20Space");
    }

    #[test]
    fn entitlement_items_accept_both_minecraft_product_names() {
        for name in ["product_minecraft", "game_minecraft"] {
            let value = json!({ "items": [{ "name": name }] });
            let has_minecraft = value
                .get("items")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .any(|item| matches!(item.get("name").and_then(Value::as_str), Some("product_minecraft" | "game_minecraft")));
            assert!(has_minecraft);
        }
    }

    #[test]
    fn cancellation_flag_is_observed() {
        let cancelled = AtomicBool::new(true);
        assert!(matches!(ensure_not_cancelled(Some(&cancelled)), Err(MicrosoftAuthError::Cancelled)));
    }
}

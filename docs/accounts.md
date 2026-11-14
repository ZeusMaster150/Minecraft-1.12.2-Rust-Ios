# Аккаунты: локальный профиль и Microsoft

Клиент поддерживает два способа входа — как PojavLauncher и другие
сторонние лаунчеры, но без встроенной формы логина и без пароля.

| Способ | Что делает | Где работает |
| --- | --- | --- |
| **Local Profile** | создаёт оффлайн-сессию по имени игрока | везде |
| **Microsoft** | официальный OAuth 2.0 **device code flow** → Xbox Live → XSTS → Minecraft Services | везде, нужен Client ID |

## 1. Локальный профиль

`Accounts → Offline` (в коде — `GuiAltCracked`).

* Пароля нет вообще: поле одно, имя игрока (3–16 символов `a-z`, `0-9`, `_`).
* Подходит для серверов с `online-mode=false`, для одиночной игры и для
  проверки рендера.
* Официальные серверы такой вход отклонят — это ограничение Mojang, а не
  клиента.
* Раньше это окно умело отправлять логин/пароль Microsoft в сторонний
  «openauth»-сервис. Эта ветка удалена: передача пароля третьей стороне нарушает
  соглашение Microsoft, и такие сервисы регулярно ломаются.

## 2. Microsoft: почему device code

На iOS нельзя открыть браузер и одновременно держать `localhost`-сервер для
callback: система приостанавливает приложение. Поэтому используется
официальный **device code flow** (RFC 8628):

```
POST /consumers/oauth2/v2.0/devicecode   -> user_code + verification_uri
игрок вводит код на microsoft.com/link
POST /consumers/oauth2/v2.0/token        -> access_token + refresh_token
      -> user.auth.xboxlive.com (XBL)
      -> xsts.auth.xboxlive.com (XSTS, RelyingParty rp://api.minecraftservices.com/)
      -> api.minecraftservices.com/authentication/login_with_xbox
      -> api.minecraftservices.com/minecraft/profile
```

Что это даёт:

* приложение **никогда не видит пароль** и не хранит его;
* не нужен встроенный браузер, WebView и callback-сервер;
* не используется чужой Client ID (Mojang/Microsoft или другого лаунчера) —
  это была бы подмена приложения и нарушение условий использования.

## 3. Свой Client ID (5 минут, бесплатно)

1. <https://portal.azure.com> → **App registrations** → **New registration**.
2. Name: любое (`MC 1.12.2 Rust`).
3. Supported account types:
   **Accounts in any organizational directory and personal Microsoft accounts**.
4. Redirect URI можно не указывать (для device code он не нужен).
5. Создать → скопировать **Application (client) ID**.
6. **Authentication** → **Allow public client flows** → **Yes** → Save.

Публичный client ID — не секрет, его можно хранить в репозитории/логе.

### Куда его положить

Порядок поиска (первый непустой выигрывает):

1. переменная окружения `MC112_MS_CLIENT_ID`;
2. значение, вшитое при сборке (`MC112_MS_CLIENT_ID` на этапе `cargo build`);
3. файл в игровой папке:
   * iOS: `Documents/config/ms-client-id.txt`
     (кинуть через Files/Finder в папку приложения);
   * десктоп: `config/ms-client-id.txt` рядом с игрой.

Для GitHub Actions: **Settings → Secrets and variables → Actions → Variables →
New repository variable**, имя `MC112_MS_CLIENT_ID`. Workflow передаёт её в
`cargo build`, и ID попадает в бинарник (`option_env!`). Без неё приложение
работает, но экран Microsoft покажет инструкцию вместо кода.

## 4. Что хранится на устройстве

`config/account.json` (совместимость со схемой Exhibition):

```json
{ "accounts": [ { "refreshToken": "...", "accessToken": "...",
                  "username": "...", "uuid": "...", "timestamp": 0 } ] }
```

* `refreshToken` — долгоживущий креденшл; именно поэтому файл теперь
  записывается с правами `0600` (только владелец), а не с зонтичными.
* На iOS файл дополнительно изолирован песочницей приложения.
* Пока это не Keychain: следующий шаг — хранить refresh-токены в
  `SecItem` (`kSecClassGenericPassword`) с `kSecAttrAccessibleAfterFirstUnlock`.
  Entitlement `keychain-access-groups` в `platform/ios/entitlements.plist` уже
  подготовлен.
* «Token» в меню аккаунтов принимает либо Minecraft access token, либо
  refresh-token (`M.C…`) и обновляет его через тот же device-code клиент.

## 5. Правовые рамки (как у PojavLauncher и Rust-лаунчеров)

* **Код игры не распространяется.** Здесь нет ни кода Mojang, ни ассетов:
  текстуры/звуки/языки подгружает сам пользователь из своей легальной копии
  (или скриптом с официального CDN Mojang — `tools/download-assets.py`).
* **Вход только официальный.** Никаких «auth-серверов», подмены сессий и
  переиспользования Client ID чужого лаунчера.
* **Имя приложения нейтральное** (`MC 1.12.2 Rust`), «Minecraft» — торговая
  марка Mojang Studios и используется только для описания совместимости;
  в UI и бандле нет логотипов и названий Mojang.
* **MoltenVK** распространяется по Apache-2.0: его `LICENSE` и
  `THIRD_PARTY_NOTICES/README.txt` кладутся в `.app` на этапе сборки.
* **Локальный профиль** — не обход лицензии: он не даёт доступа к купленному
  контенту и не работает на онлайн-серверах.

## 6. Диагностика

| Симптом | Причина |
| --- | --- |
| «This build has no Microsoft application id configured» | нет Client ID (см. §3) |
| `AADSTS7000218` | в Azure не включён **Allow public client flows** |
| `AADSTS700016` / `unauthorized_client` | Client ID от другого типа приложения или опечатка |
| `authorization_declined` | вход отменён на странице Microsoft |
| `expired_token` | код не ввели за 15 минут — нажмите Microsoft ещё раз |
| `login_with_xbox` → 401/403 | у аккаунта нет Xbox-профиля или нет Java Edition |
| «no profile found» | у аккаунта нет Java-профиля; имя задаётся в диалоге (Windows) |

Логи: `Documents/latest.log` на iOS, консоль на десктопе.

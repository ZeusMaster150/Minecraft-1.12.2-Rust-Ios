use crate::net::minecraft::client::gui::FontRenderer::FontRenderer;
use crate::net::minecraft::client::gui::GuiButton::{GuiButton, GuiSoundCommand};
use crate::net::minecraft::client::gui::GuiScreen::GuiScreen;
use crate::net::minecraft::client::gui::GuiTextField::{
    GuiTextField, GuiTextFieldKey, GuiTextFieldModifiers,
};
use crate::net::minecraft::util::Session::Session;
use crate::vulkan::GuiDrawList::GuiDrawList;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiAltCrackedAction {
    Cancel,
    Authenticated(Session),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiAltCrackedInteraction {
    pub action: GuiAltCrackedAction,
    pub sound: Option<GuiSoundCommand>,
}

/// Local (offline) profile selection.
///
/// This screen only ever creates a local session: it asks for a player name and
/// nothing else. There is deliberately no password field and no credential
/// exchange here. Server-side login only works for servers that run in
/// `online-mode=false`; official servers reject offline sessions, and sending a
/// Microsoft password to a third-party login form would violate the Microsoft
/// services agreement (and is exactly what this screen used to do). Real Microsoft
/// accounts sign in through `GuiMicrosoftAuth`, which uses the official device
/// code flow.
#[derive(Debug)]
pub struct GuiAltCracked {
    pub GuiScreen: GuiScreen,
    username: GuiTextField,
    status: String,
}

impl GuiAltCracked {
    pub fn new() -> Self {
        Self {
            GuiScreen: GuiScreen::default(),
            username: GuiTextField::new(0, 0, 0, 200, 20),
            status: "§7Local profile - no password needed§r".to_owned(),
        }
    }

    pub fn initGui(&mut self, width: i32, height: i32) {
        self.GuiScreen.width = width;
        self.GuiScreen.height = height;
        self.GuiScreen.buttonList.clear();
        let y = height / 4 + 24;
        self.GuiScreen
            .buttonList
            .push(GuiButton::new(0, width / 2 - 100, y + 84, "Log In"));
        self.GuiScreen
            .buttonList
            .push(GuiButton::new(1, width / 2 - 100, y + 108, "Back"));
        self.username = GuiTextField::new(0, width / 2 - 100, 100, 200, 20);
        self.username.setMaxStringLength(16);
        self.username.setFocused(true);
    }

    pub fn updateScreen(&mut self) -> Option<Session> {
        self.username.updateCursorCounter();
        None
    }

    pub fn drawScreen(
        &mut self,
        drawList: &mut GuiDrawList,
        font: &mut FontRenderer,
        mouseX: i32,
        mouseY: i32,
        partialTicks: f32,
    ) {
        self.GuiScreen.drawDefaultBackground(drawList);
        self.username.drawTextBox(drawList, font);
        self.GuiScreen.Gui.drawCenteredString(
            font,
            drawList,
            "Local Profile",
            self.GuiScreen.width / 2,
            20,
            -1,
        );
        self.GuiScreen.Gui.drawCenteredString(
            font,
            drawList,
            &self.status,
            self.GuiScreen.width / 2,
            32,
            -1,
        );
        if self.username.getText().is_empty() {
            self.GuiScreen.Gui.drawString(
                font,
                drawList,
                "Player name (3-16 of a-z, 0-9, _)",
                self.GuiScreen.width / 2 - 96,
                106,
                -7_829_368,
            );
        }
        self.GuiScreen
            .drawScreen(drawList, font, mouseX, mouseY, partialTicks);
    }

    pub fn mouseClicked(
        &mut self,
        mouseX: i32,
        mouseY: i32,
        mouseButton: i32,
        font: &FontRenderer,
    ) -> Option<GuiAltCrackedInteraction> {
        self.username
            .mouseClicked(mouseX, mouseY, mouseButton, font);
        if mouseButton != 0 {
            return None;
        }
        let button = self
            .GuiScreen
            .buttonList
            .iter()
            .find(|button| button.mousePressed(mouseX, mouseY))?
            .clone();
        let action = match button.id {
            1 => GuiAltCrackedAction::Cancel,
            0 => self.login(),
            _ => GuiAltCrackedAction::None,
        };
        Some(GuiAltCrackedInteraction {
            action,
            sound: Some(button.playPressSound()),
        })
    }

    pub fn typedText(&mut self, text: &str, font: &FontRenderer) -> bool {
        if self.username.isFocused() {
            self.username.writeText(text, Some(font))
        } else {
            false
        }
    }

    pub fn keyPressed(
        &mut self,
        key: GuiTextFieldKey,
        modifiers: GuiTextFieldModifiers,
        font: &FontRenderer,
    ) -> bool {
        if self.username.isFocused() {
            self.username.keyPressed(key, modifiers, font)
        } else {
            false
        }
    }

    pub fn selectAll(&mut self, font: &FontRenderer) {
        if self.username.isFocused() {
            self.username.selectAll(font);
        }
    }

    pub fn tabPressed(&mut self) {
        // Only one input field remains, so keep the caret in it.
        self.username.setFocused(true);
    }

    pub fn enterPressed(&mut self) -> GuiAltCrackedAction {
        self.login()
    }

    /// Normalises the entered name to the character set Minecraft accepts for
    /// offline play. Invalid characters are dropped rather than rejected so a
    /// mobile keyboard cannot produce an unusable profile.
    fn normalise_name(raw: &str) -> String {
        raw.chars()
            .filter(|character| character.is_ascii_alphanumeric() || *character == '_')
            .take(16)
            .collect()
    }

    fn login(&mut self) -> GuiAltCrackedAction {
        let username = Self::normalise_name(&self.username.getText());
        if username.is_empty() {
            self.status = "§cEnter a player name§r".to_owned();
            return GuiAltCrackedAction::None;
        }
        let session = Session::new(username.clone(), "", "", "mojang");
        self.status = format!("§aSelected profile: {username}§r");
        GuiAltCrackedAction::Authenticated(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_username_is_rejected_instead_of_creating_a_blank_profile() {
        let mut screen = GuiAltCracked::new();
        screen.initGui(854, 480);
        assert_eq!(screen.enterPressed(), GuiAltCrackedAction::None);
    }

    #[test]
    fn entered_name_becomes_an_offline_session() {
        let mut screen = GuiAltCracked::new();
        screen.initGui(854, 480);
        let name = "Steve_01";
        screen.username = GuiTextField::new(0, 0, 0, 200, 20);
        screen.username.setText(name);
        match screen.enterPressed() {
            GuiAltCrackedAction::Authenticated(session) => {
                assert_eq!(session.getUsername(), name);
                assert!(session.getToken().is_empty());
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn invalid_characters_are_removed() {
        assert_eq!(GuiAltCracked::normalise_name("St eve! #1"), "Steve1");
    }

    #[test]
    fn name_is_capped_at_sixteen_characters() {
        assert_eq!(GuiAltCracked::normalise_name(&"a".repeat(40)).len(), 16);
    }
}

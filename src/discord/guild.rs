use super::Guild;

impl Guild {
    pub fn icon_url(&self) -> Option<String> {
        self.icon.as_ref().map(|icon| format!(
                "https://cdn.discordapp.com/icons/{}/{}.png",
                self.id, icon
            ))
    }
}

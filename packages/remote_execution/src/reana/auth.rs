use dialoguer::{Input, theme::ColorfulTheme};
use keyring::Entry;
use std::error::Error;

pub fn login_reana() -> Result<(String, String), Box<dyn Error>> {
    let reana_instance = get_or_prompt_credential("reana", "instance", "Enter REANA instance URL: ")?;
    let reana_token = get_or_prompt_credential("reana", "token", "Enter REANA access token: ")?;
    Ok((reana_instance, reana_token))
}

pub fn logout_reana() -> Result<(), Box<dyn Error>> {
    Entry::new("reana", "instance")?.delete_credential()?;
    Entry::new("reana", "token")?.delete_credential()?;
    eprintln!("✅ Successfully logged out from previous REANA instances.");
    Ok(())
}

fn get_or_prompt_credential(service: &str, key: &str, prompt: &str) -> Result<String, Box<dyn Error>> {
    let entry = Entry::new(service, key)?;
    match entry.get_password() {
        Ok(val) => Ok(val),
        Err(keyring::Error::NoEntry) => {
            let input: String = Input::with_theme(&ColorfulTheme::default()).with_prompt(prompt).interact_text()?;
            let value = input.trim().to_string();
            entry.set_password(&value)?;
            Ok(value)
        }
        Err(e) => Err(Box::new(e)),
    }
}

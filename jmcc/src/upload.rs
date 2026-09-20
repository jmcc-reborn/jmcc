//! Module uploading service via official `JustMC` API or Discord webhook.

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::process::{Command, Stdio};

const OFFICIAL_DOMAIN: &str = "https://m.justmc.ru";
const OFFICIAL_UPLOAD_URL: &str = "https://m.justmc.ru/api/upload";
const MAX_WEBHOOK_SIZE: usize = 8 * 1024 * 1024; // 8 MB

/// Upload target service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum UploadTarget {
    /// Official `JustMC` service (`https://m.justmc.ru/api/upload`).
    #[default]
    Official,
    /// Discord webhook.
    Webhook,
}

impl std::fmt::Display for UploadTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Official => write!(f, "official"),
            Self::Webhook => write!(f, "webhook"),
        }
    }
}

impl std::str::FromStr for UploadTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "official" | "justmc" | "default" => Ok(Self::Official),
            "webhook" | "discord" => Ok(Self::Webhook),
            other => Err(format!(
                "Unknown upload target '{other}'. Allowed values: 'official' (or 'justmc'), 'webhook' (or 'discord')"
            )),
        }
    }
}

/// Uploads compiled JSON module via the selected target service.
///
/// # Errors
/// Returns error if network transfer fails or server returns error response.
#[expect(clippy::print_stdout, reason = "user-facing upload instructions")]
pub fn upload_module(
    json_str: &str,
    target: UploadTarget,
    custom_webhook: Option<&str>,
) -> color_eyre::Result<String> {
    let file_url = match target {
        UploadTarget::Official => upload_official(json_str)?,
        UploadTarget::Webhook => upload_webhook(json_str, custom_webhook)?,
    };

    match crate::i18n::current_lang() {
        crate::i18n::Lang::Ru => {
            println!("\x1b[32mФайл успешно загружен!\x1b[0m\n");
            println!("\x1b[90mИспользуйте данную команду на сервере для загрузки модуля:\x1b[0m");
            println!("\x1b[34m/module loadUrl force {file_url}\x1b[0m\n");
            println!(
                "\x1b[33mВажно:\x1b[0m \x1b[37mМодуль по ссылке удалится через \x1b[31m3 минуты\x1b[37m!\x1b[0m"
            );
            println!(
                "      \x1b[37mУспейте использовать команду на сервере за данное время.\x1b[0m"
            );
        }
        crate::i18n::Lang::En => {
            println!("\x1b[32mThe file has been uploaded successfully!\x1b[0m\n");
            println!("\x1b[90mUse this command on the server to load the module:\x1b[0m");
            println!("\x1b[34m/module loadUrl force {file_url}\x1b[0m\n");
            println!(
                "\x1b[33mImportant:\x1b[0m \x1b[37mThe linked module will be deleted in \x1b[31m3 minutes\x1b[37m!\x1b[0m"
            );
            println!(
                "          \x1b[37mSucceed in using the command on the server in the given time.\x1b[0m"
            );
        }
    }

    Ok(file_url)
}

fn upload_official(json_str: &str) -> color_eyre::Result<String> {
    let mut child = Command::new("curl")
        .arg("-s")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-H")
        .arg("User-Agent: JMCC-Compiler")
        .arg("--data-binary")
        .arg("@-")
        .arg(OFFICIAL_UPLOAD_URL)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to execute curl: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json_str.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        color_eyre::eyre::bail!("Failed to upload module via curl: {err}");
    }

    let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| {
        color_eyre::eyre::eyre!(
            "Invalid JSON response from upload server ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })?;

    if let Some(err) = resp_json.get("error").and_then(|e| e.as_str()) {
        color_eyre::eyre::bail!("Server returned upload error: {err}");
    }

    let id = resp_json
        .get("id")
        .and_then(|i| i.as_str())
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("Upload response does not contain 'id': {resp_json}")
        })?;

    Ok(format!("{OFFICIAL_DOMAIN}/api/{id}"))
}

fn upload_webhook(json_str: &str, custom_webhook: Option<&str>) -> color_eyre::Result<String> {
    let webhook_url = custom_webhook.ok_or_else(|| match crate::i18n::current_lang() {
        crate::i18n::Lang::Ru => color_eyre::eyre::eyre!(
            "URL вебхука не указан. Укажите его через флаг --webhook-url или в jmcc.toml"
        ),
        crate::i18n::Lang::En => color_eyre::eyre::eyre!(
            "Webhook URL is not configured. Please specify it via --webhook-url or in jmcc.toml"
        ),
    })?;
    let data_size = json_str.len();
    if data_size > MAX_WEBHOOK_SIZE {
        match crate::i18n::current_lang() {
            crate::i18n::Lang::Ru => {
                color_eyre::eyre::bail!(
                    "Файл слишком большой: {data_size} байт (максимум 8 MB для Discord)"
                );
            }
            crate::i18n::Lang::En => {
                color_eyre::eyre::bail!(
                    "File is too large: {data_size} bytes (maximum 8 MB for Discord)"
                );
            }
        }
    }

    let mut child = Command::new("curl")
        .arg("-s")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("User-Agent: JMCC-Compiler")
        .arg("-F")
        .arg("file=@-;filename=code.txt;type=text/plain;charset=utf-8")
        .arg(webhook_url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to execute curl: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json_str.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        color_eyre::eyre::bail!("Failed to upload module via curl: {err}");
    }

    let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let file_url = resp_json
        .get("attachments")
        .and_then(|a| a.get(0))
        .and_then(|att| att.get("url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("Discord response does not contain attachment URL: {resp_json}")
        })?;

    Ok(file_url.to_string())
}

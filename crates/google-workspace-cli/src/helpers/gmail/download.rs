// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use regex::Regex;
use std::fs;
use std::path::PathBuf;

/// Handle the `+download-attachments` subcommand.
pub(super) async fn handle_download_attachments(
    _doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let message_id = matches.get_one::<String>("id").unwrap();
    let pattern = matches.get_one::<String>("pattern");
    let output_dir_str = matches.get_one::<String>("output-dir");
    let output_dir = output_dir_str
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let dry_run = matches.get_flag("dry-run");

    if let Some(dir) = output_dir_str {
        crate::validate::validate_safe_output_dir(dir)?;
    }

    let t = auth::get_token(&[GMAIL_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // 1. Fetch Metadata
    let original = if dry_run {
        OriginalMessage::dry_run_placeholder(message_id)
    } else {
        fetch_message_metadata(&client, &t, message_id).await?
    };

    // 2. Compile Regex if pattern provided
    let regex = pattern
        .map(|p| Regex::new(p).context("Invalid regex pattern"))
        .transpose()?;

    // 3. Create output directory if it doesn't exist
    if !dry_run && !output_dir.exists() {
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;
    }

    let mut download_count = 0;

    for part in original.parts {
        if part.is_inline() {
            continue;
        }

        if should_download_part(&part.filename, regex.as_ref()) {
            let mut target_path = output_dir.clone();
            target_path.push(&part.filename);

            if dry_run {
                println!(
                    "[dry-run] Would download: {} ({} bytes) to {:?}",
                    part.filename, part.size, target_path
                );
            } else {
                println!("Downloading: {} ({} bytes)...", part.filename, part.size);

                let data =
                    fetch_attachment_data(&client, &t, message_id, &part.attachment_id).await?;

                fs::write(&target_path, data)
                    .with_context(|| format!("Failed to write file: {:?}", target_path))?;
            }
            download_count += 1;
        }
    }

    if download_count == 0 {
        println!("No matching attachments found.");
    } else {
        println!("Successfully processed {download_count} attachment(s).");
    }

    Ok(())
}

fn should_download_part(filename: &str, regex: Option<&Regex>) -> bool {
    match regex {
        Some(re) => re.is_match(filename),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_download_part_no_pattern() {
        assert!(should_download_part("test.pdf", None));
        assert!(should_download_part("image.png", None));
    }

    #[test]
    fn test_should_download_part_with_pattern() {
        let re = Regex::new(r".*\.pdf").unwrap();
        assert!(should_download_part("test.pdf", Some(&re)));
        assert!(!should_download_part("image.png", Some(&re)));

        let re2 = Regex::new(r"^Invoice").unwrap();
        assert!(should_download_part("Invoice_123.pdf", Some(&re2)));
        assert!(!should_download_part("My_Invoice.pdf", Some(&re2)));
    }
}

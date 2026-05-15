//! Extract urls from M3U playlist files

// TODO: resolve relative paths

use base64::Engine;

use super::PlaylistValue;

#[derive(Debug, Clone, PartialEq)]
pub struct M3UItem {
    pub url: PlaylistValue,
}

/// M3U(8) is a de-facto standard (meaning there is no formal standard), where each line that does not start with `#` is a entry, separated by newlines
///
/// <https://en.wikipedia.org/wiki/M3U#File_format>
///
/// `#EXTINF:` metadata lines are parsed and the title/artist/duration are
/// packed into the following entry's URL fragment as `tmeta=BASE64(...)`
/// so they can be recovered by [`Track::new_radio`]. Path entries cannot
/// carry a fragment so their EXTINF metadata is dropped.
pub fn decode(content: &str) -> Vec<M3UItem> {
    let lines = content.lines();
    let mut list = vec![];
    let mut pending_meta: Option<ExtInfMeta> = None;

    for line in lines {
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            pending_meta = Some(parse_extinf(rest));
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let mut p_value = match PlaylistValue::try_from_str(line) {
            Ok(v) => v,
            Err(err) => {
                warn!("Failed to parse url / path, ignoring! Error: {err:#?}");
                pending_meta = None;
                continue;
            }
        };
        if let Err(err) = p_value.file_url_to_path() {
            warn!("Failed to convert file:// url to path, ignoring! Error: {err:#?}");
            pending_meta = None;
            continue;
        }

        if let (Some(meta), PlaylistValue::Url(url)) = (pending_meta.take(), &mut p_value) {
            meta.attach_to_url(url);
        }

        list.push(M3UItem { url: p_value });
    }
    list
}

#[derive(Debug, Default)]
struct ExtInfMeta {
    duration_sec: Option<i64>,
    title: Option<String>,
    artist: Option<String>,
}

/// Parse the body of an `#EXTINF:` line.
///
/// Format: `<seconds>,<label>` where `<label>` is either `Artist - Title`
/// or just `Title`. Seconds <= 0 are ignored (m3u uses -1 for unknown).
fn parse_extinf(rest: &str) -> ExtInfMeta {
    let (dur_str, label) = rest.split_once(',').unwrap_or((rest, ""));
    let duration_sec = dur_str.trim().parse::<i64>().ok().filter(|d| *d > 0);

    let label = label.trim();
    let (artist, title) = if let Some(idx) = label.find(" - ") {
        let a = label[..idx].trim();
        let t = label[idx + 3..].trim();
        let a = (!a.is_empty()).then(|| a.to_string());
        let t = (!t.is_empty()).then(|| t.to_string());
        (a, t)
    } else if label.is_empty() {
        (None, None)
    } else {
        (None, Some(label.to_string()))
    };

    ExtInfMeta {
        duration_sec,
        title,
        artist,
    }
}

impl ExtInfMeta {
    /// Pack metadata into the URL fragment as `tmeta=BASE64(tab-separated)`
    /// so [`Track::new_radio`] can recover it. Empty fields are kept as
    /// empty strings to preserve column positions.
    fn attach_to_url(&self, url: &mut reqwest::Url) {
        if self.title.is_none() && self.artist.is_none() && self.duration_sec.is_none() {
            return;
        }
        let line = format!(
            "{}\t{}\t{}",
            self.title.as_deref().unwrap_or(""),
            self.artist.as_deref().unwrap_or(""),
            self.duration_sec
                .map(|d| d.to_string())
                .unwrap_or_default(),
        );
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(line.as_bytes());
        url.set_fragment(Some(&format!("tmeta={b64}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use reqwest::Url;

    #[test]
    fn should_parse() {
        let playlist = r"/some/absolute/unix/path.mp3
# this is a test comment, below is a empty line to be ignored

relative.mp3
https://somewhere.url/path";

        let results = decode(playlist);
        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0].url,
            PlaylistValue::Path("/some/absolute/unix/path.mp3".into())
        );
        assert_eq!(results[1].url, PlaylistValue::Path("relative.mp3".into()));
        assert_eq!(
            results[2].url,
            PlaylistValue::Url(Url::parse("https://somewhere.url/path").unwrap())
        );
    }
}

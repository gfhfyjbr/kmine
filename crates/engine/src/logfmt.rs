use quick_xml::Reader;
use quick_xml::events::Event;

pub struct LogDecoder {
    buf: String,
}

impl Default for LogDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl LogDecoder {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    pub fn push_line(&mut self, line: &str) -> Vec<String> {
        if self.buf.is_empty() && !is_xml_fragment(line) {
            let text = line.trim_end();
            return if text.is_empty() {
                Vec::new()
            } else {
                vec![text.to_string()]
            };
        }
        if !self.buf.is_empty() {
            self.buf.push('\n');
        }
        self.buf.push_str(line);
        drain_ready(&mut self.buf)
    }

    pub fn finish(&mut self) -> Vec<String> {
        let leftover = std::mem::take(&mut self.buf);
        let leftover = leftover.trim();
        if leftover.is_empty() || is_xml_noise(leftover) {
            Vec::new()
        } else {
            vec![leftover.to_string()]
        }
    }
}

fn drain_ready(buf: &mut String) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        let Some((from, to)) = event_span(buf) else {
            if buf.len() > 64 * 1024 {
                let leftover = std::mem::take(buf);
                if !is_xml_noise(leftover.trim()) {
                    out.push(leftover);
                }
            }
            break;
        };
        if from > 0 {
            let raw = buf[..from].trim().to_string();
            if !raw.is_empty() && !is_xml_noise(&raw) {
                out.push(raw);
            }
        }
        let xml = buf[from..to].to_string();
        buf.drain(..to);
        if let Some(lines) = format_log4j_event(&xml) {
            out.extend(lines);
        } else if !is_xml_noise(xml.trim()) {
            out.push(xml);
        }
    }
    out
}

fn is_xml_fragment(line: &str) -> bool {
    let text = line.trim_start();
    text.starts_with("<log4j:")
        || text.starts_with("<Event ")
        || text.starts_with("<Event>")
        || text.starts_with("<?xml")
}

fn is_xml_noise(text: &str) -> bool {
    let text = text.trim();
    text.is_empty()
        || text.starts_with("<?xml")
        || text.starts_with("<log4j:EventSet")
        || text == "</log4j:EventSet>"
}

fn event_span(buf: &str) -> Option<(usize, usize)> {
    let start = buf
        .find("<log4j:Event")
        .or_else(|| buf.find("<Event "))
        .or_else(|| buf.find("<Event>"))?;
    let after = &buf[start + 1..];
    let name_end = after.find([' ', '>'])?;
    let name = &after[..name_end];
    if name != "Event" && name != "log4j:Event" {
        return None;
    }
    let close = format!("</{name}>");
    let rel = buf[start..].find(&close)?;
    Some((start, start + rel + close.len()))
}

fn format_log4j_event(xml: &str) -> Option<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut level = String::new();
    let mut logger = String::new();
    let mut message = String::new();
    let mut throwable = String::new();
    let mut in_message = false;
    let mut in_throwable = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(tag)) | Ok(Event::Empty(tag)) => {
                let name = tag.local_name();
                if name.as_ref() == b"Event" {
                    for attr in tag.attributes().flatten() {
                        let key = attr.key.local_name();
                        let value = attr.unescape_value().ok()?.into_owned();
                        match key.as_ref() {
                            b"level" => level = value,
                            b"logger" => logger = value,
                            _ => {}
                        }
                    }
                } else if name.as_ref() == b"Message" {
                    in_message = true;
                } else if name.as_ref() == b"Throwable" {
                    in_throwable = true;
                }
            }
            Ok(Event::CData(data)) => {
                let text = data.decode().ok()?;
                append_text(
                    &mut message,
                    &mut throwable,
                    in_message,
                    in_throwable,
                    &text,
                );
            }
            Ok(Event::Text(text)) => {
                let decoded = text.decode().ok()?;
                append_text(
                    &mut message,
                    &mut throwable,
                    in_message,
                    in_throwable,
                    &decoded,
                );
            }
            Ok(Event::End(tag)) => {
                let name = tag.local_name();
                if name.as_ref() == b"Message" {
                    in_message = false;
                } else if name.as_ref() == b"Throwable" {
                    in_throwable = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    if message.is_empty() && throwable.is_empty() {
        return None;
    }
    if !throwable.is_empty() {
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&throwable);
    }
    Some(format_entry(&level, &logger, &message))
}

fn append_text(
    message: &mut String,
    throwable: &mut String,
    in_message: bool,
    in_throwable: bool,
    text: &str,
) {
    if in_message {
        message.push_str(text);
    } else if in_throwable {
        throwable.push_str(text);
    }
}

fn format_entry(level: &str, logger: &str, message: &str) -> Vec<String> {
    let level = if level.is_empty() { "INFO" } else { level };
    let logger = short_logger(logger);
    let mut lines = message.lines();
    let first = lines.next().unwrap_or("").trim_end();
    let head = if logger.is_empty() {
        format!("[{level}] {first}")
    } else {
        format!("[{level}] {logger}: {first}")
    };
    let mut out = vec![head];
    for line in lines {
        out.push(format!("       {line}"));
    }
    out
}

fn short_logger(logger: &str) -> &str {
    if logger.len() <= 48 {
        logger
    } else {
        logger
            .rsplit(['.', '/'])
            .next()
            .filter(|part| !part.is_empty())
            .unwrap_or(logger)
    }
}

#[cfg(test)]
mod tests {
    use super::LogDecoder;

    #[test]
    fn passes_through_plain_lines() {
        let mut dec = LogDecoder::new();
        assert_eq!(
            dec.push_line("OpenJDK 64-Bit Server VM"),
            vec!["OpenJDK 64-Bit Server VM"]
        );
    }

    #[test]
    fn parses_split_log4j_event() {
        let mut dec = LogDecoder::new();
        assert!(dec
            .push_line(r#"<log4j:Event logger="FabricLoader/GameProvider" timestamp="1" level="INFO" thread="main">"#)
            .is_empty());
        assert!(dec
            .push_line(r#"<log4j:Message><![CDATA[Loading Minecraft 1.21.1 with Fabric Loader 0.19.3]]></log4j:Message>"#)
            .is_empty());
        assert_eq!(
            dec.push_line("</log4j:Event>"),
            vec![
                "[INFO] FabricLoader/GameProvider: Loading Minecraft 1.21.1 with Fabric Loader 0.19.3"
            ]
        );
    }

    #[test]
    fn splits_multiline_cdata() {
        let xml = r#"<log4j:Event logger="FabricLoader" level="INFO" thread="main">
<log4j:Message><![CDATA[Loading 4 mods:
	 - fabricloader 0.19.3
	 - minecraft 1.21.1]]></log4j:Message>
</log4j:Event>"#;
        let mut dec = LogDecoder::new();
        let lines = dec.push_line(xml);
        assert_eq!(lines[0], "[INFO] FabricLoader: Loading 4 mods:");
        assert_eq!(lines[1], "       \t - fabricloader 0.19.3");
        assert_eq!(lines[2], "       \t - minecraft 1.21.1");
    }

    #[test]
    fn ignores_eventset_noise() {
        let mut dec = LogDecoder::new();
        assert!(dec.push_line("<?xml version=\"1.0\"?>").is_empty());
        assert!(dec.push_line("<log4j:EventSet>").is_empty());
    }
}

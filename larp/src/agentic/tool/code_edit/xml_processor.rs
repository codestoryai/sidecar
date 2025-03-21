pub struct XmlProcessor {
    buffer: String,
    processed_up_to: usize,
}

impl XmlProcessor {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            processed_up_to: 0,
        }
    }

    /// Appends new content to the buffer.
    pub fn append(&mut self, new_content: &str) {
        self.buffer.push_str(new_content);
    }

    /// Extracts the content of a specific tag from the buffer.
    pub fn extract_tag_content(&self, tag_name: &str) -> Option<String> {
        let tag_start = format!("<{}>", tag_name);
        let tag_end = format!("</{}>", tag_name);

        let start_index = self.buffer.find(&tag_start)?;
        let content_start = start_index.checked_add(tag_start.len())?;

        self.buffer
            .get(content_start..)?
            .find(&tag_end)
            .and_then(|end_index| {
                let content_end = content_start.checked_add(end_index)?;
                self.buffer
                    .get(content_start..content_end)
                    .map(|content| content.to_string())
            })
    }

    /// Extracts all contents of a specific tag from the buffer, starting from the last processed position.
    pub fn extract_all_tag_contents(&mut self, tag_name: &str) -> Vec<String> {
        let tag_start = format!("<{}>", tag_name);
        let tag_end = format!("</{}>", tag_name);
        let mut contents = Vec::new();
        let mut pos: usize = self.processed_up_to;

        while let Some(start_index) = self.buffer[pos..].find(&tag_start) {
            let start_index = pos + start_index;
            let search_start = start_index + tag_start.len();
            if let Some(end_index) = self.buffer[search_start..].find(&tag_end) {
                let content_start = search_start;
                let content_end = search_start + end_index;
                if let Some(content) = self.buffer.get(content_start..content_end) {
                    contents.push(content.to_string());
                } else {
                    println!("xmlprocessor::extract_all_tag_contents::buffer.get() - utf-related panic prevented");
                    break;
                }
                pos = content_end + tag_end.len();
                self.processed_up_to = pos;
            } else {
                break;
            }
        }

        contents
    }

    /// Wraps raw XML content within a root tag.
    pub fn wrap_xml(root_tag: &str, raw_xml: &str) -> String {
        format!("<{root_tag}>{raw_xml}</{root_tag}>")
    }
}

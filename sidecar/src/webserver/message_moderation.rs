//! Contains functionality for moderating chat messages to filter out inappropriate content

use std::collections::HashSet;
use once_cell::sync::Lazy;

/// A static set of words that are considered inappropriate and should be filtered
static INAPPROPRIATE_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    let mut set = HashSet::new();
    // Common profanity and offensive terms
    set.insert("fuck");
    set.insert("shit");
    set.insert("ass");
    set.insert("bitch");
    set.insert("damn");
    set.insert("cunt");
    set.insert("dick");
    set.insert("cock");
    set.insert("pussy");
    // Add more words as needed
    set
});

/// Checks if a message contains inappropriate content
/// 
/// # Arguments
/// 
/// * `message` - The message to check
/// 
/// # Returns
/// 
/// `true` if the message contains inappropriate content, `false` otherwise
pub fn contains_inappropriate_content(message: &str) -> bool {
    let lowercase_message = message.to_lowercase();
    
    // Check if any inappropriate word is in the message
    INAPPROPRIATE_WORDS.iter().any(|&word| lowercase_message.contains(word))
}

/// Filters inappropriate content from a message
/// 
/// # Arguments
/// 
/// * `message` - The message to filter
/// 
/// # Returns
/// 
/// A filtered version of the message with inappropriate content replaced with asterisks
pub fn filter_inappropriate_content(message: &str) -> String {
    let mut filtered_message = message.to_string();
    
    for &word in INAPPROPRIATE_WORDS.iter() {
        let replacement = "*".repeat(word.len());
        
        // Case-insensitive replacement
        let lowercase_message = filtered_message.to_lowercase();
        let mut start_idx = 0;
        
        while let Some(pos) = lowercase_message[start_idx..].find(word) {
            let actual_pos = start_idx + pos;
            filtered_message.replace_range(actual_pos..actual_pos + word.len(), &replacement);
            start_idx = actual_pos + replacement.len();
            
            // Break if we've reached the end of the string
            if start_idx >= lowercase_message.len() {
                break;
            }
        }
    }
    
    filtered_message
}

/// Determines if a message should be blocked entirely due to inappropriate content
/// 
/// # Arguments
/// 
/// * `message` - The message to check
/// 
/// # Returns
/// 
/// `true` if the message should be blocked, `false` otherwise
pub fn should_block_message(message: &str) -> bool {
    // Block messages that are primarily inappropriate content
    // This is a simple implementation that blocks messages where more than 30% of the words are inappropriate
    
    let words: Vec<&str> = message.split_whitespace().collect();
    if words.is_empty() {
        return false;
    }
    
    let inappropriate_word_count = words.iter()
        .filter(|&&word| INAPPROPRIATE_WORDS.contains(&word.to_lowercase()))
        .count();
    
    // Block if more than 30% of words are inappropriate
    (inappropriate_word_count as f32 / words.len() as f32) > 0.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_inappropriate_content() {
        assert!(contains_inappropriate_content("This is a fuck test"));
        assert!(contains_inappropriate_content("This is a FUCK test"));
        assert!(contains_inappropriate_content("This is a Fuck test"));
        assert!(!contains_inappropriate_content("This is a clean test"));
    }

    #[test]
    fn test_filter_inappropriate_content() {
        assert_eq!(filter_inappropriate_content("This is a fuck test"), "This is a **** test");
        assert_eq!(filter_inappropriate_content("This is a FUCK test"), "This is a **** test");
        assert_eq!(filter_inappropriate_content("This is a Fuck test"), "This is a **** test");
        assert_eq!(filter_inappropriate_content("This is a clean test"), "This is a clean test");
    }

    #[test]
    fn test_should_block_message() {
        assert!(should_block_message("fuck you"));
        assert!(should_block_message("fuck shit damn"));
        assert!(!should_block_message("This is a test with one inappropriate word: fuck"));
        assert!(!should_block_message("This is a clean test"));
    }
}
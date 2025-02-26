# Sidecar Testing Guide

This guide explains how to effectively test Sidecar components and contribute new tests.

## Testing Architecture

### Backend Tests

1. **Unit Tests**
   - Located alongside source files
   - Test individual components
   - Mock external dependencies

2. **Integration Tests**
   - Located in `tests/` directory
   - Test component interactions
   - Use test fixtures

3. **End-to-End Tests**
   - Test complete workflows
   - Simulate real user scenarios

## Test Categories

### 1. LLM Client Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_completion_request() {
        let client = MockLLMClient::new();
        let response = client.complete("fn main() {").await;
        assert!(response.is_ok());
    }
}
```

### 2. Repository Analysis Tests
```rust
#[test]
fn test_symbol_extraction() {
    let code = "function hello() { return 'world'; }";
    let symbols = extract_symbols(code);
    assert_eq!(symbols[0].name, "hello");
}
```

### 3. API Tests
```rust
#[tokio::test]
async fn test_completion_endpoint() {
    let app = create_test_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/completion")
                .body(Body::from(r#"{"code":"fn test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
```

## Test Utilities

### 1. Mock LLM Client
```rust
pub struct MockLLMClient {
    responses: HashMap<String, String>,
}

impl LLMClient for MockLLMClient {
    async fn complete(&self, prompt: &str) -> Result<String> {
        Ok(self.responses.get(prompt).unwrap_or_default().clone())
    }
}
```

### 2. Test Fixtures
```rust
pub fn setup_test_repo() -> PathBuf {
    let dir = tempdir().unwrap();
    // Initialize git repo
    // Add test files
    dir.path().to_path_buf()
}
```

## Running Tests

1. **All Tests:**
```bash
cargo test
```

2. **Specific Test:**
```bash
cargo test test_name
```

3. **Test with Logging:**
```bash
RUST_LOG=debug cargo test
```

## Writing Tests

### 1. Unit Test Template
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_functionality() {
        // Arrange
        let input = ...;

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }
}
```

### 2. Integration Test Template
```rust
use sidecar::{Client, Config};

#[tokio::test]
async fn test_workflow() {
    // Setup
    let client = Client::new(Config::test());

    // Execute workflow
    let result = client.process_file("test.rs").await?;

    // Verify
    assert!(result.is_success());
}
```

## Test Coverage

1. **Running Coverage:**
```bash
cargo tarpaulin
```

2. **Coverage Goals:**
   - Core components: 90%+
   - Utilities: 80%+
   - Integration points: 70%+

## Mocking External Services

1. **HTTP Services:**
```rust
use wiremock::{Mock, ResponseTemplate};

Mock::given(method("POST"))
    .and(path("/v1/completions"))
    .respond_with(ResponseTemplate::new(200).body_json(&mock_response))
    .mount(&mock_server)
    .await;
```

2. **File System:**
```rust
use tempfile::TempDir;

let temp_dir = TempDir::new()?;
let test_file = temp_dir.path().join("test.rs");
std::fs::write(test_file, "test content")?;
```

## Testing Best Practices

1. **Test Organization:**
   - Group related tests
   - Use descriptive names
   - Follow AAA pattern (Arrange-Act-Assert)

2. **Test Data:**
   - Use meaningful test data
   - Avoid large fixtures
   - Clean up test resources

3. **Assertions:**
   - Make specific assertions
   - Test edge cases
   - Handle async operations properly

## Common Testing Scenarios

1. **Testing LLM Responses:**
   - Mock API responses
   - Test token handling
   - Verify error cases

2. **Testing File Operations:**
   - Use temporary directories
   - Test file permissions
   - Handle cleanup

3. **Testing WebSocket:**
   - Mock connections
   - Test message handling
   - Verify reconnection logic

## Debugging Tests

1. **Logging:**
```rust
use env_logger;

#[test]
fn test_with_logs() {
    env_logger::init();
    // Test code
}
```

2. **Test Failures:**
   - Check test output
   - Use debug assertions
   - Add logging statements

## CI/CD Integration

1. **GitHub Actions:**
```yaml
test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v2
    - name: Run tests
      run: cargo test
```

2. **Coverage Reports:**
   - Generate reports
   - Set minimum thresholds
   - Track trends
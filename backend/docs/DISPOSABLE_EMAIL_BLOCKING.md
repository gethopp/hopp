# Disposable Email Blocking Feature

## Overview

This feature prevents users from registering with disposable/temporary email addresses, helping to reduce noise in the user base and prevent abuse. The system maintains a comprehensive list of known disposable email domains and validates email addresses during registration.

## Features

- **Comprehensive Domain List**: Blocks over 700 known disposable email domains
- **Configurable**: Can be enabled/disabled via environment variables
- **Strict Mode**: Optional mode that only allows business domains
- **Subdomain Detection**: Detects subdomains of disposable services (e.g., `test.mailinator.com`)
- **Case Insensitive**: Works regardless of email case
- **User-Friendly Errors**: Provides helpful error messages and suggestions

## Configuration

Add these environment variables to control the behavior:

```bash
# Enable/disable disposable email blocking (default: true)
BLOCK_DISPOSABLE_EMAILS=true

# Enable strict mode - only allow business domains (default: false)  
EMAIL_STRICT_MODE=false
```

## Implementation Details

### Files Added/Modified

1. **`internal/utils/disposable_domains.go`** - Comprehensive list of disposable domains
2. **`internal/utils/email_validator.go`** - Core validation logic
3. **`internal/utils/email_validator_test.go`** - Comprehensive test suite
4. **`internal/config/config.go`** - Configuration options
5. **`internal/handlers/handlers.go`** - Integration with signup handlers
6. **`internal/models/user.go`** - Model updates

### Validation Flow

1. **Format Validation**: Basic email format check using Go's `mail.ParseAddress`
2. **Domain Extraction**: Extract domain from email address
3. **Disposable Check**: Check against comprehensive domain list
4. **Business Email Check**: Optional strict mode validation
5. **Error Response**: User-friendly error with suggestions

### API Response Format

When a disposable email is detected, the API returns:

```json
{
  "error": "Email from disposable domain 'mailinator.com' is not allowed. Please use a permanent email address.",
  "error_code": "DISPOSABLE_EMAIL",
  "suggestions": [
    "Use your work email address",
    "Use a personal email from Gmail, Yahoo, or Outlook",
    "Use your organization's email domain",
    "Create a permanent email account with a major email provider"
  ]
}
```

## Usage Examples

### Basic Usage (Default Configuration)

```go
// This will block disposable emails by default
err := utils.ValidateEmailAddress("test@10minutemail.com")
if err != nil {
    // Handle error - will be DISPOSABLE_EMAIL
}
```

### Custom Configuration

```go
cfg := &utils.EmailValidationConfig{
    BlockDisposableEmails: true,
    StrictMode:           false,
}

err := utils.ValidateEmailAddressWithConfig("test@gmail.com", cfg)
```

### Strict Mode Example

```go
cfg := &utils.EmailValidationConfig{
    BlockDisposableEmails: true,
    StrictMode:           true,  // Only business domains allowed
}

// This will fail with NON_BUSINESS_EMAIL
err := utils.ValidateEmailAddressWithConfig("test@gmail.com", cfg)
```

## Blocked Domains

The system blocks popular disposable email services including:

- **Temporary Email Services**: 10minutemail.com, tempmail.org, temp-mail.ru
- **Guerrilla Mail Services**: guerrillamail.com, guerrillamail.net
- **Mailinator Services**: mailinator.com, mailinator.net
- **Throwaway Services**: throwaway.email, trashmail.com
- **And 700+ more...**

## Testing

Run the comprehensive test suite:

```bash
go test ./internal/utils/... -v
```

The test suite includes:
- Unit tests for all validation functions
- Configuration testing
- Error handling validation
- Benchmark tests for performance
- Edge case testing

## Performance

The validation is highly optimized:
- O(1) domain lookup using Go maps
- Minimal string processing
- Efficient subdomain checking
- Benchmarked for high throughput

## Monitoring & Analytics

The system logs registration attempts with disposable emails for monitoring:

```go
c.Logger().Infof("Blocked disposable email registration attempt: %s", email)
```

## Future Enhancements

1. **Dynamic Domain Updates**: Regularly update domain list from external sources
2. **Whitelist Support**: Allow specific disposable domains if needed
3. **Analytics Dashboard**: Track blocked registration attempts
4. **Custom Domain Rules**: Organization-specific domain policies
5. **Machine Learning**: Detect new disposable patterns

## Troubleshooting

### Common Issues

1. **Legitimate Email Blocked**: 
   - Check if domain was incorrectly added to disposable list
   - Consider adding to whitelist (future feature)

2. **Configuration Not Working**:
   - Verify environment variables are set correctly
   - Check configuration loading in `config.go`

3. **Performance Issues**:
   - Domain list lookup is O(1), should be very fast
   - Run benchmarks to identify bottlenecks

### Debug Mode

Enable debug logging to trace validation:

```go
c.Logger().Debugf("Email validation - Domain: %s, IsDisposable: %v", domain, isDisposable)
```

## Contributing

To add new disposable domains:

1. Update `internal/utils/disposable_domains.go`
2. Add domain to the map with `true` value
3. Add test cases in `email_validator_test.go`
4. Run tests to verify

## Security Considerations

- Domain list is maintained in source code for performance
- No external API calls during validation
- Case-insensitive matching prevents bypass attempts
- Subdomain detection prevents subdomain abuse

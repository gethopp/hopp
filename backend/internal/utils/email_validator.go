package utils

import (
	"fmt"
	"net/mail"
	"strings"
)

// EmailValidationError represents an error during email validation
type EmailValidationError struct {
	Message string
	Code    string
}

func (e EmailValidationError) Error() string {
	return e.Message
}

// ValidateEmailAddress validates an email address and checks if it's from a disposable email service
// Pass nil for config to use default behavior (block disposable emails)
func ValidateEmailAddress(email string) error {
	return ValidateEmailAddressWithConfig(email, nil)
}

// EmailValidationConfig holds configuration for email validation
type EmailValidationConfig struct {
	BlockDisposableEmails bool
	StrictMode           bool
}

// ValidateEmailAddressWithConfig validates an email address with configuration options
func ValidateEmailAddressWithConfig(email string, cfg *EmailValidationConfig) error {
	// Use default config if none provided
	if cfg == nil {
		cfg = &EmailValidationConfig{
			BlockDisposableEmails: true,
			StrictMode:           false,
		}
	}

	// Basic email format validation
	_, err := mail.ParseAddress(email)
	if err != nil {
		return &EmailValidationError{
			Message: "Invalid email format",
			Code:    "INVALID_FORMAT",
		}
	}

	// Extract domain from email
	domain, err := extractDomain(email)
	if err != nil {
		return &EmailValidationError{
			Message: "Could not extract domain from email",
			Code:    "DOMAIN_EXTRACTION_ERROR",
		}
	}

	// Check if domain is disposable (only if enabled)
	if cfg.BlockDisposableEmails && IsDisposableEmail(domain) {
		return &EmailValidationError{
			Message: fmt.Sprintf("Email from disposable domain '%s' is not allowed. Please use a permanent email address.", domain),
			Code:    "DISPOSABLE_EMAIL",
		}
	}

	// Check strict mode (only business domains)
	if cfg.StrictMode && !IsValidBusinessEmail(email) {
		return &EmailValidationError{
			Message: "Please use a business email address for registration.",
			Code:    "NON_BUSINESS_EMAIL",
		}
	}

	return nil
}

// IsDisposableEmail checks if an email domain is in the disposable domains list
func IsDisposableEmail(domain string) bool {
	// Convert to lowercase for case-insensitive comparison
	domain = strings.ToLower(strings.TrimSpace(domain))
	
	// Check exact domain match
	if DisposableEmailDomains[domain] {
		return true
	}

	// Check for subdomain matches (e.g., test.mailinator.com should match mailinator.com)
	parts := strings.Split(domain, ".")
	if len(parts) > 2 {
		// Check parent domain (e.g., for "test.mailinator.com", check "mailinator.com")
		parentDomain := strings.Join(parts[len(parts)-2:], ".")
		if DisposableEmailDomains[parentDomain] {
			return true
		}
	}

	return false
}

// extractDomain extracts the domain part from an email address
func extractDomain(email string) (string, error) {
	email = strings.TrimSpace(email)
	parts := strings.Split(email, "@")
	if len(parts) != 2 {
		return "", fmt.Errorf("invalid email format")
	}
	return strings.ToLower(parts[1]), nil
}

// IsValidBusinessEmail checks if an email appears to be from a business domain
// This is a heuristic check and not foolproof, but helps identify legitimate business emails
func IsValidBusinessEmail(email string) bool {
	domain, err := extractDomain(email)
	if err != nil {
		return false
	}

	// Common personal email domains that we might want to flag
	personalDomains := map[string]bool{
		"gmail.com":        true,
		"yahoo.com":        true,
		"yahoo.co.uk":      true,
		"yahoo.fr":         true,
		"yahoo.de":         true,
		"hotmail.com":      true,
		"hotmail.co.uk":    true,
		"hotmail.fr":       true,
		"hotmail.de":       true,
		"outlook.com":      true,
		"outlook.co.uk":    true,
		"outlook.fr":       true,
		"outlook.de":       true,
		"aol.com":          true,
		"icloud.com":       true,
		"me.com":           true,
		"mac.com":          true,
		"protonmail.com":   true,
		"protonmail.ch":    true,
		"tutanota.com":     true,
		"tutanota.de":      true,
	}

	// If it's a disposable domain, definitely not business
	if IsDisposableEmail(domain) {
		return false
	}

	// If it's a common personal domain, it's probably not business (but we allow it)
	// This function can be used for analytics or optional warnings
	return !personalDomains[domain]
}

// SuggestAlternatives provides suggestions when a disposable email is detected
func SuggestAlternatives() []string {
	return []string{
		"Use your work email address",
		"Use a personal email from Gmail, Yahoo, or Outlook",
		"Use your organization's email domain",
		"Create a permanent email account with a major email provider",
	}
}

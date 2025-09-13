package utils

import (
	"testing"
)

func TestIsDisposableEmail(t *testing.T) {
	tests := []struct {
		name     string
		domain   string
		expected bool
	}{
		{"Known disposable domain", "10minutemail.com", true},
		{"Known disposable domain case insensitive", "MAILINATOR.COM", true},
		{"Subdomain of disposable domain", "test.mailinator.com", true},
		{"Legitimate business domain", "microsoft.com", false},
		{"Legitimate personal domain", "gmail.com", false},
		{"Empty domain", "", false},
		{"Non-existent domain", "thisdoesnotexist123456.com", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := IsDisposableEmail(tt.domain)
			if result != tt.expected {
				t.Errorf("IsDisposableEmail(%s) = %v; expected %v", tt.domain, result, tt.expected)
			}
		})
	}
}

func TestExtractDomain(t *testing.T) {
	tests := []struct {
		name     string
		email    string
		expected string
		hasError bool
	}{
		{"Valid email", "test@example.com", "example.com", false},
		{"Valid email with subdomain", "user@mail.example.com", "mail.example.com", false},
		{"Email with uppercase", "TEST@EXAMPLE.COM", "example.com", false},
		{"Email with spaces", "  test@example.com  ", "example.com", false},
		{"Invalid email - no @", "testexample.com", "", true},
		{"Invalid email - multiple @", "test@@example.com", "", true},
		{"Empty email", "", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := extractDomain(tt.email)
			if tt.hasError {
				if err == nil {
					t.Errorf("extractDomain(%s) expected error but got none", tt.email)
				}
			} else {
				if err != nil {
					t.Errorf("extractDomain(%s) unexpected error: %v", tt.email, err)
				}
				if result != tt.expected {
					t.Errorf("extractDomain(%s) = %s; expected %s", tt.email, result, tt.expected)
				}
			}
		})
	}
}

func TestValidateEmailAddress(t *testing.T) {
	tests := []struct {
		name        string
		email       string
		expectError bool
		errorCode   string
	}{
		{"Valid business email", "john@microsoft.com", false, ""},
		{"Valid personal email", "john@gmail.com", false, ""},
		{"Disposable email", "test@10minutemail.com", true, "DISPOSABLE_EMAIL"},
		{"Another disposable email", "user@mailinator.com", true, "DISPOSABLE_EMAIL"},
		{"Case insensitive disposable", "USER@YOPMAIL.COM", true, "DISPOSABLE_EMAIL"},
		{"Invalid email format", "notanemail", true, "INVALID_FORMAT"},
		{"Empty email", "", true, "INVALID_FORMAT"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateEmailAddress(tt.email)
			if tt.expectError {
				if err == nil {
					t.Errorf("ValidateEmailAddress(%s) expected error but got none", tt.email)
				} else {
					if emailErr, ok := err.(*EmailValidationError); ok {
						if emailErr.Code != tt.errorCode {
							t.Errorf("ValidateEmailAddress(%s) error code = %s; expected %s", tt.email, emailErr.Code, tt.errorCode)
						}
					} else {
						t.Errorf("ValidateEmailAddress(%s) expected EmailValidationError but got %T", tt.email, err)
					}
				}
			} else {
				if err != nil {
					t.Errorf("ValidateEmailAddress(%s) unexpected error: %v", tt.email, err)
				}
			}
		})
	}
}

func TestValidateEmailAddressWithConfig(t *testing.T) {
	tests := []struct {
		name        string
		email       string
		config      *EmailValidationConfig
		expectError bool
		errorCode   string
	}{
		{
			name:        "Disposable email with blocking enabled",
			email:       "test@10minutemail.com",
			config:      &EmailValidationConfig{BlockDisposableEmails: true, StrictMode: false},
			expectError: true,
			errorCode:   "DISPOSABLE_EMAIL",
		},
		{
			name:        "Disposable email with blocking disabled",
			email:       "test@10minutemail.com",
			config:      &EmailValidationConfig{BlockDisposableEmails: false, StrictMode: false},
			expectError: false,
			errorCode:   "",
		},
		{
			name:        "Personal email with strict mode enabled",
			email:       "test@gmail.com",
			config:      &EmailValidationConfig{BlockDisposableEmails: true, StrictMode: true},
			expectError: true,
			errorCode:   "NON_BUSINESS_EMAIL",
		},
		{
			name:        "Personal email with strict mode disabled",
			email:       "test@gmail.com",
			config:      &EmailValidationConfig{BlockDisposableEmails: true, StrictMode: false},
			expectError: false,
			errorCode:   "",
		},
		{
			name:        "Business email with strict mode enabled",
			email:       "john@microsoft.com",
			config:      &EmailValidationConfig{BlockDisposableEmails: true, StrictMode: true},
			expectError: false,
			errorCode:   "",
		},
		{
			name:        "Nil config uses defaults",
			email:       "test@10minutemail.com",
			config:      nil,
			expectError: true,
			errorCode:   "DISPOSABLE_EMAIL",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateEmailAddressWithConfig(tt.email, tt.config)
			if tt.expectError {
				if err == nil {
					t.Errorf("ValidateEmailAddressWithConfig(%s) expected error but got none", tt.email)
				} else {
					if emailErr, ok := err.(*EmailValidationError); ok {
						if emailErr.Code != tt.errorCode {
							t.Errorf("ValidateEmailAddressWithConfig(%s) error code = %s; expected %s", tt.email, emailErr.Code, tt.errorCode)
						}
					} else {
						t.Errorf("ValidateEmailAddressWithConfig(%s) expected EmailValidationError but got %T", tt.email, err)
					}
				}
			} else {
				if err != nil {
					t.Errorf("ValidateEmailAddressWithConfig(%s) unexpected error: %v", tt.email, err)
				}
			}
		})
	}
}

func TestIsValidBusinessEmail(t *testing.T) {
	tests := []struct {
		name     string
		email    string
		expected bool
	}{
		{"Business domain", "john@microsoft.com", true},
		{"Another business domain", "jane@apple.com", true},
		{"Personal Gmail", "user@gmail.com", false},
		{"Personal Yahoo", "user@yahoo.com", false},
		{"Personal Hotmail", "user@hotmail.com", false},
		{"Personal Outlook", "user@outlook.com", false},
		{"Disposable email", "user@10minutemail.com", false},
		{"Invalid email format", "notanemail", false},
		{"Custom domain (business)", "user@mycompany.io", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := IsValidBusinessEmail(tt.email)
			if result != tt.expected {
				t.Errorf("IsValidBusinessEmail(%s) = %v; expected %v", tt.email, result, tt.expected)
			}
		})
	}
}

func TestSuggestAlternatives(t *testing.T) {
	suggestions := SuggestAlternatives()
	if len(suggestions) == 0 {
		t.Error("SuggestAlternatives() should return non-empty slice")
	}
	
	expectedSuggestions := []string{
		"Use your work email address",
		"Use a personal email from Gmail, Yahoo, or Outlook",
		"Use your organization's email domain",
		"Create a permanent email account with a major email provider",
	}
	
	if len(suggestions) != len(expectedSuggestions) {
		t.Errorf("SuggestAlternatives() returned %d suggestions; expected %d", len(suggestions), len(expectedSuggestions))
	}
	
	for i, expected := range expectedSuggestions {
		if i < len(suggestions) && suggestions[i] != expected {
			t.Errorf("SuggestAlternatives()[%d] = %s; expected %s", i, suggestions[i], expected)
		}
	}
}

func TestEmailValidationError(t *testing.T) {
	err := &EmailValidationError{
		Message: "Test error message",
		Code:    "TEST_CODE",
	}
	
	if err.Error() != "Test error message" {
		t.Errorf("EmailValidationError.Error() = %s; expected 'Test error message'", err.Error())
	}
}

// Benchmark tests
func BenchmarkIsDisposableEmail(b *testing.B) {
	testDomains := []string{
		"10minutemail.com",
		"gmail.com",
		"microsoft.com",
		"mailinator.com",
		"unknowndomain.com",
	}
	
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		domain := testDomains[i%len(testDomains)]
		IsDisposableEmail(domain)
	}
}

func BenchmarkValidateEmailAddress(b *testing.B) {
	testEmails := []string{
		"test@10minutemail.com",
		"user@gmail.com",
		"john@microsoft.com",
		"invalid-email",
		"user@mailinator.com",
	}
	
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		email := testEmails[i%len(testEmails)]
		ValidateEmailAddress(email)
	}
}

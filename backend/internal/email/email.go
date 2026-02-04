package email

import (
	"fmt"
	"hopp-backend/internal/models"
	"os"
	"strings"

	"github.com/labstack/echo/v4"
	resend "github.com/resend/resend-go/v2"
)

// EmailClient is an interface for sending emails
type EmailClient interface {
	SendAsync(toEmail, subject, htmlBody string)
	SendWelcomeEmail(user *models.User)
	SendPasswordResetEmail(toEmail, resetToken string)
	SendTeamInvitationEmail(inviterName, teamName, inviteLink, toEmail string)
	SendTeamRemovalEmail(user *models.User, oldTeamName, newTeamName string)
	SendSubscriptionConfirmationEmail(user *models.User)
	SendSubscriptionCancellationEmail(user *models.User)
	SendInvoiceEmail(toEmail string, data InvoiceEmailData)
}

// InvoiceEmailData contains the data needed to send an invoice email
type InvoiceEmailData struct {
	// e.g. "AFFUTBBC-1234"
	InvoiceNumber string
	// e.g. "Jan 3 - Feb 3, 2026"
	Period string
	// e.g. "https://invoice.stripe.com/i/aa..."
	HostedInvoiceURL string
	// e.g. "https://pay.stripe.com/invoice/..."
	InvoicePDFURL string
}

// ResendEmailClient implements EmailClient using the Resend service
type ResendEmailClient struct {
	client        *resend.Client
	defaultSender string
	logger        echo.Logger
}

// NewResendEmailClient creates a new ResendEmailClient
func NewResendEmailClient(client *resend.Client, defaultSender string, logger echo.Logger) *ResendEmailClient {
	return &ResendEmailClient{
		client:        client,
		defaultSender: defaultSender,
		logger:        logger,
	}
}

// SendAsync sends an email asynchronously
func (c *ResendEmailClient) SendAsync(toEmail, subject, htmlBody string) {
	if c == nil || c.client == nil {
		fmt.Println("Resend client not initialized, skipping email.")
		return
	}

	if c.defaultSender == "" {
		c.logger.Errorf("Resend default sender not configured, skipping email.")
		return
	}

	go func() {
		params := &resend.SendEmailRequest{
			From:    c.defaultSender,
			To:      []string{toEmail},
			Subject: subject,
			Html:    htmlBody,
		}

		_, err := c.client.Emails.Send(params)
		if err != nil {
			// Replace with proper logging
			c.logger.Errorf("Failed to send email to %s (Subject: %s): %v\n", toEmail, subject, err)
		} else {
			// Replace with proper logging
			c.logger.Infof("Email sent successfully to %s (Subject: %s)\n", toEmail, subject)
		}
	}()
}

// SendWelcomeEmail sends a welcome email to a new user
func (c *ResendEmailClient) SendWelcomeEmail(user *models.User) {
	if user == nil {
		c.logger.Error("Cannot send welcome email to nil user")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-welcome.html")
	if err != nil {
		c.logger.Errorf("Failed to read welcome email template: %v", err)
		return
	}

	htmlBody := strings.ReplaceAll(string(templateBytes), "{first_name}", user.FirstName)
	subject := "Welcome to Hopp " + user.FirstName

	c.SendAsync(user.Email, subject, htmlBody)
}

func (c *ResendEmailClient) SendPasswordResetEmail(toEmail, resetLink string) {
	if toEmail == "" || resetLink == "" {
		c.logger.Error("Cannot send password reset email with empty email or link")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-password-reset.html")
	if err != nil {
		c.logger.Errorf("Failed to read password reset email template: %v", err)
		return
	}

	htmlBody := strings.ReplaceAll(string(templateBytes), "{reset_link}", resetLink)

	if err := os.WriteFile("testing.html", []byte(htmlBody), 0644); err != nil {
		c.logger.Errorf("Failed to write testing.html: %v", err)
	}

	subject := "Hopp Password Reset Request"

	c.SendAsync(toEmail, subject, htmlBody)
}

// SendTeamInvitationEmail sends an invitation email to join a team
func (c *ResendEmailClient) SendTeamInvitationEmail(inviterName, teamName, inviteLink, toEmail string) {
	if c == nil || c.client == nil {
		fmt.Println("Resend client not initialized, skipping email.")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-invite-teammate.html")
	if err != nil {
		c.logger.Errorf("Failed to read team invitation email template: %v", err)
		return
	}

	htmlBody := string(templateBytes)
	htmlBody = strings.ReplaceAll(htmlBody, "{inviter_name}", inviterName)
	htmlBody = strings.ReplaceAll(htmlBody, "{team_name}", teamName)
	htmlBody = strings.ReplaceAll(htmlBody, "{invite_url}", inviteLink)

	subject := fmt.Sprintf("%s has invited you to join %s team - join the team", inviterName, teamName)

	c.SendAsync(toEmail, subject, htmlBody)
}

// SendTeamRemovalEmail sends an email to a user who has been removed from a team
func (c *ResendEmailClient) SendTeamRemovalEmail(user *models.User, oldTeamName, newTeamName string) {
	if user == nil {
		c.logger.Error("Cannot send team removal email to nil user")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-team-removed.html")
	if err != nil {
		c.logger.Errorf("Failed to read team removal email template: %v", err)
		return
	}

	replacer := strings.NewReplacer(
		"{first_name}", user.FirstName,
		"{old_team_name}", oldTeamName,
		"{new_team_name}", newTeamName,
	)
	htmlBody := replacer.Replace(string(templateBytes))

	subject := fmt.Sprintf("Hopp: You've been removed from %s", oldTeamName)

	c.SendAsync(user.Email, subject, htmlBody)
}

// SendSubscriptionConfirmationEmail sends a subscription confirmation email
func (c *ResendEmailClient) SendSubscriptionConfirmationEmail(user *models.User) {
	if user == nil {
		c.logger.Error("Cannot send subscription confirmation email to nil user")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-subscription.html")
	if err != nil {
		c.logger.Errorf("Failed to read subscription confirmation email template: %v", err)
		return
	}

	htmlBody := strings.ReplaceAll(string(templateBytes), "{first_name}", user.FirstName)

	subject := "Welcome to Hopp Pro! 🎉"

	c.SendAsync(user.Email, subject, htmlBody)
}

// SendSubscriptionCancellationEmail sends a subscription cancellation email
func (c *ResendEmailClient) SendSubscriptionCancellationEmail(user *models.User) {
	if user == nil {
		c.logger.Error("Cannot send subscription cancellation email to nil user")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-unsubscribe.html")
	if err != nil {
		c.logger.Errorf("Failed to read subscription cancellation email template: %v", err)
		return
	}

	htmlBody := strings.ReplaceAll(string(templateBytes), "{first_name}", user.FirstName)

	subject := "We're sorry to see you go 😢"

	c.SendAsync(user.Email, subject, htmlBody)
}

// SendInvoiceEmail sends an invoice email with links to view and download the invoice
func (c *ResendEmailClient) SendInvoiceEmail(toEmail string, data InvoiceEmailData) {
	if toEmail == "" {
		c.logger.Error("Cannot send invoice email to empty email address")
		return
	}

	// Read the template file
	templateBytes, err := os.ReadFile("web/emails/hopp-invoice.html")
	if err != nil {
		c.logger.Errorf("Failed to read invoice email template: %v", err)
		return
	}

	replacer := strings.NewReplacer(
		"{invoice_number}", data.InvoiceNumber,
		"{period}", data.Period,
		"{view_link}", data.HostedInvoiceURL,
		"{download_link}", data.InvoicePDFURL,
	)
	htmlBody := replacer.Replace(string(templateBytes))

	subject := fmt.Sprintf("Invoice #%s from Hopp 🏀", data.InvoiceNumber)

	c.SendAsync(toEmail, subject, htmlBody)
}

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
	SendTeamInvitationEmail(inviterName, teamName, inviteLink, toEmail string)
	SendSubscriptionConfirmationEmail(user *models.User)
	SendSubscriptionCancellationEmail(user *models.User)
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

	htmlBody := strings.Replace(string(templateBytes), "{first_name}", user.FirstName, -1)
	subject := "Welcome to Hopp " + user.FirstName

	c.SendAsync(user.Email, subject, htmlBody)
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
	htmlBody = strings.Replace(htmlBody, "{inviter_name}", inviterName, -1)
	htmlBody = strings.Replace(htmlBody, "{team_name}", teamName, -1)
	htmlBody = strings.Replace(htmlBody, "{invite_url}", inviteLink, -1)

	subject := fmt.Sprintf("%s has invited you to join %s team - join the team", inviterName, teamName)

	c.SendAsync(toEmail, subject, htmlBody)
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

	htmlBody := strings.Replace(string(templateBytes), "{first_name}", user.FirstName, -1)

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

	htmlBody := strings.Replace(string(templateBytes), "{first_name}", user.FirstName, -1)

	subject := "We're sorry to see you go 😢"

	c.SendAsync(user.Email, subject, htmlBody)
}

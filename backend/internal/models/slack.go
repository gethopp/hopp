package models

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"time"

	"gorm.io/gorm"
)

// SlackInstallation stores workspace-level bot tokens for Slack app installations.
// This is used for posting messages via chat.postMessage and receiving /hopp commands.
// Each installation is linked to a Hopp team via TeamID.
type SlackInstallation struct {
	ID             uint      `json:"id" gorm:"primaryKey"`
	SlackTeamID    string    `json:"slack_team_id" gorm:"uniqueIndex;not null"`
	SlackTeamName  string    `json:"slack_team_name"`
	BotAccessToken string    `json:"-" gorm:"not null"` // Encrypted at rest
	BotUserID      string    `json:"bot_user_id"`
	Scopes         string    `json:"scopes"`
	InstalledAt    time.Time `json:"installed_at"`
	InstalledByID  string    `json:"installed_by_id"` // Hopp user ID who installed
	TeamID         *uint     `json:"team_id" gorm:"index"`
	Team           *Team     `json:"team,omitempty"`
	CreatedAt      time.Time `json:"created_at"`
	UpdatedAt      time.Time `json:"updated_at"`
}

// GetSlackInstallationByTeamID retrieves the Slack installation for a Slack workspace by Slack Team ID.
func GetSlackInstallationByTeamID(db *gorm.DB, slackTeamID string) (*SlackInstallation, error) {
	var installation SlackInstallation
	result := db.Where("slack_team_id = ?", slackTeamID).First(&installation)
	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, nil
		}
		return nil, result.Error
	}
	return &installation, nil
}

// GetSlackInstallationByHoppTeamID retrieves the Slack installation linked to a Hopp team.
func GetSlackInstallationByHoppTeamID(db *gorm.DB, hoppTeamID uint) (*SlackInstallation, error) {
	var installation SlackInstallation
	result := db.Where("team_id = ?", hoppTeamID).First(&installation)
	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, nil
		}
		return nil, result.Error
	}
	return &installation, nil
}

// DeleteSlackInstallation deletes a Slack installation by ID.
func DeleteSlackInstallation(db *gorm.DB, id uint) error {
	return db.Delete(&SlackInstallation{}, id).Error
}

// EncryptToken encrypts a token using AES-GCM with the provided key.
// The key should be 32 bytes for AES-256.
func EncryptToken(plaintext string, keyBase64 string) (string, error) {
	if keyBase64 == "" {
		// If no encryption key configured, store plaintext (not recommended for production)
		return plaintext, nil
	}

	key, err := base64.StdEncoding.DecodeString(keyBase64)
	if err != nil {
		return "", fmt.Errorf("invalid encryption key: %w", err)
	}

	block, err := aes.NewCipher(key)
	if err != nil {
		return "", fmt.Errorf("creating cipher: %w", err)
	}

	aesGCM, err := cipher.NewGCM(block)
	if err != nil {
		return "", fmt.Errorf("creating GCM: %w", err)
	}

	nonce := make([]byte, aesGCM.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", fmt.Errorf("generating nonce: %w", err)
	}

	ciphertext := aesGCM.Seal(nonce, nonce, []byte(plaintext), nil)
	return base64.StdEncoding.EncodeToString(ciphertext), nil
}

// DecryptToken decrypts a token that was encrypted with EncryptToken.
func DecryptToken(ciphertextBase64 string, keyBase64 string) (string, error) {
	if keyBase64 == "" {
		// If no encryption key configured, assume plaintext
		return ciphertextBase64, nil
	}

	key, err := base64.StdEncoding.DecodeString(keyBase64)
	if err != nil {
		return "", fmt.Errorf("invalid encryption key: %w", err)
	}

	ciphertext, err := base64.StdEncoding.DecodeString(ciphertextBase64)
	if err != nil {
		return "", fmt.Errorf("invalid ciphertext: %w", err)
	}

	block, err := aes.NewCipher(key)
	if err != nil {
		return "", fmt.Errorf("creating cipher: %w", err)
	}

	aesGCM, err := cipher.NewGCM(block)
	if err != nil {
		return "", fmt.Errorf("creating GCM: %w", err)
	}

	nonceSize := aesGCM.NonceSize()
	if len(ciphertext) < nonceSize {
		return "", errors.New("ciphertext too short")
	}

	nonce, ciphertext := ciphertext[:nonceSize], ciphertext[nonceSize:]
	plaintext, err := aesGCM.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return "", fmt.Errorf("decrypting: %w", err)
	}

	return string(plaintext), nil
}

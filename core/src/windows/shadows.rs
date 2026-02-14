//! Shadow tokens based on Tailwind/design system shadows
use iced::{Color, Shadow, Vector};

/// Shadow tokens for consistent shadow usage across the app
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShadowToken {
    /// Extra small shadow - subtle depth
    /// CSS: 0px 1px 2px rgba(10, 13, 18, 0.05)
    Xs,
    /// Small shadow
    /// CSS: 0px 1px 3px rgba(10, 13, 18, 0.1)
    Sm,
    /// Medium shadow
    /// CSS: 0px 4px 6px -1px rgba(10, 13, 18, 0.1)
    Md,
    /// Large shadow
    /// CSS: 0px 12px 16px -4px rgba(10, 13, 18, 0.08)
    Lg,
    /// Extra large shadow
    /// CSS: 0px 20px 24px -4px rgba(10, 13, 18, 0.08)
    Xl,
    /// 2XL shadow
    /// CSS: 0px 24px 48px -12px rgba(10, 13, 18, 0.18)
    Xxl,
    /// 3XL shadow - maximum depth
    /// CSS: 0px 32px 64px -12px rgba(10, 13, 18, 0.14)
    Xxxl,
    /// No shadow
    None,
}

impl ShadowToken {
    /// Convert the shadow token to an iced Shadow
    pub fn to_shadow(&self) -> Shadow {
        // Base shadow color: rgba(10, 13, 18, opacity)
        // #0A0D12 = rgb(10, 13, 18)
        let base_r = 10.0 / 255.0;
        let base_g = 13.0 / 255.0;
        let base_b = 18.0 / 255.0;

        match self {
            ShadowToken::Xs => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.05),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            },
            ShadowToken::Sm => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.1),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 3.0,
            },
            ShadowToken::Md => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.1),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 6.0,
            },
            ShadowToken::Lg => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.08),
                offset: Vector::new(0.0, 12.0),
                blur_radius: 16.0,
            },
            ShadowToken::Xl => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.08),
                offset: Vector::new(0.0, 20.0),
                blur_radius: 24.0,
            },
            ShadowToken::Xxl => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.18),
                offset: Vector::new(0.0, 24.0),
                blur_radius: 48.0,
            },
            ShadowToken::Xxxl => Shadow {
                color: Color::from_rgba(base_r, base_g, base_b, 0.14),
                offset: Vector::new(0.0, 32.0),
                blur_radius: 64.0,
            },
            ShadowToken::None => Shadow::default(),
        }
    }
}

impl From<ShadowToken> for Shadow {
    fn from(token: ShadowToken) -> Self {
        token.to_shadow()
    }
}

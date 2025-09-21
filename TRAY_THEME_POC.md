# Tray Icon Theme Switching POC

This POC demonstrates how to automatically switch tray icons based on the macOS system theme (Light/Dark mode).

## 📁 File Structure

### Tray Icons Location

The custom tray icons should be placed in:

```
tauri/src-tauri/resources/tray-icons/
├── tray-light.png    # Icon for light theme
└── tray-dark.png     # Icon for dark theme
```

Currently, these are just copies of the existing app icon. **Replace these with your custom icons:**

- `tray-light.png` - Icon optimized for light menu bars (typically darker)
- `tray-dark.png` - Icon optimized for dark menu bars (typically lighter)

### Icon Requirements

- **Format**: PNG
- **Size**: 32x32 pixels (or higher with @2x variants)
- **Style**:
  - Light theme icon should be dark/black for visibility on light menu bars
  - Dark theme icon should be light/white for visibility on dark menu bars

## 🔧 Implementation Details

### Core Components

1. **Theme Detection Module** (`src/theme.rs`)

   - Detects current macOS system theme using `defaults` command
   - Monitors theme changes in background thread
   - Cross-platform fallback for non-macOS systems

2. **Tray Icon Management** (`src/lib.rs`)

   - `setup_tray_icon()` - Initializes tray with theme-appropriate icon
   - `update_tray_icon_for_theme()` - Updates icon when theme changes
   - Automatic theme monitoring with callback system

3. **Test Commands** (`src/main.rs`)
   - `get_current_theme` - Returns current system theme
   - `test_tray_icon_switch` - Manually switch icons for testing

### Frontend Test Interface

A test component is available in the Debug tab of the app:

- Shows current system theme
- Buttons to manually test light/dark icon switching
- Real-time theme detection

## 🚀 How to Use

### 1. Replace Icon Assets

Replace the placeholder icons with your custom designs:

```bash
# Replace with your custom icons
cp your-light-icon.png tauri/src-tauri/resources/tray-icons/tray-light.png
cp your-dark-icon.png tauri/src-tauri/resources/tray-icons/tray-dark.png
```

### 2. Build and Run

```bash
cd tauri
npm run tauri dev  # For development
# or
npm run tauri build  # For production
```

### 3. Test the Feature

#### Automatic Testing:

1. Run the app
2. Change your macOS system theme (System Preferences → General → Appearance)
3. Watch the tray icon change automatically

#### Manual Testing:

1. Open the app
2. Navigate to the Debug tab (bug icon in sidebar)
3. Use the "Tray Icon Theme Test" section
4. Click "Switch to Light" or "Switch to Dark" buttons
5. Check your menu bar to see the icon change

## 🔍 Monitoring and Debugging

### Logs

The app logs theme changes and icon updates:

```
System theme changed to: Dark
Failed to update tray icon: [error details]
```

### Commands Available

- `get_current_theme()` - Get current system theme
- `test_tray_icon_switch(theme)` - Manually switch to "light" or "dark"

## 📝 Technical Notes

### Theme Detection Method

Currently uses the `defaults` command to check `AppleInterfaceStyle`:

- Returns "Dark" for dark mode
- Returns error/empty for light mode
- Polls every second for changes

### Limitations

- macOS only (graceful fallback on other platforms)
- Manual test command doesn't persist (automatic detection overrides)
- Requires proper icon assets to see visual difference

### Future Improvements

- Use native macOS APIs for more efficient theme detection
- Add support for system accent colors
- Implement icon caching for better performance
- Add support for template icons that auto-adapt

## 🎨 Icon Design Tips

### Light Theme Icon (tray-light.png)

- Use dark colors (black, dark gray)
- Ensure good contrast against light menu bars
- Consider macOS menu bar height (~22px)

### Dark Theme Icon (tray-dark.png)

- Use light colors (white, light gray)
- Ensure good contrast against dark menu bars
- Match the same dimensions as light version

### Best Practices

- Keep designs simple and recognizable at small sizes
- Use monochromatic colors for better system integration
- Test on both Intel and Apple Silicon Macs
- Consider different screen densities (@1x, @2x)

## 🐛 Troubleshooting

### Icon Not Changing

1. Check if icons exist in `resources/tray-icons/`
2. Verify icon format (PNG) and permissions
3. Check logs for error messages
4. Try manual test commands first

### Theme Detection Not Working

1. Ensure running on macOS
2. Check system permissions
3. Verify `defaults` command works in terminal
4. Look for theme monitoring errors in logs

---

This POC provides a solid foundation for theme-aware tray icons. Customize the icons and extend the functionality as needed for your application.

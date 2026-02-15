# BISC-047: macOS Packaging — .app Bundle & .dmg

**Phase**: 9 — Platform Support
**Depends on**: BISC-045

## Problem Statement

macOS apps distributed outside the App Store require code signing and notarization, or users get Gatekeeper warnings. No `.app` bundle structure, `Info.plist`, or signing workflow exists.

## Deliverables

### 1. .app Bundle Structure

Create a build script or `Makefile` target that produces:

```
bisc.app/
  Contents/
    MacOS/
      bisc          (the binary)
    Resources/
      bisc.icns     (app icon)
    Info.plist      (bundle metadata)
```

- `Info.plist`: bundle identifier (`com.bisc.app`), version, minimum macOS version (13+ for ScreenCaptureKit)
- App icon: placeholder `.icns` file (can be replaced later with real branding)

### 2. Universal Binary (Optional)

- Build for both `x86_64-apple-darwin` and `aarch64-apple-darwin`
- Combine with `lipo` into a universal binary
- Or distribute separate arm64/x86_64 builds

### 3. Code Signing

- Document the signing process using `codesign` or `apple-codesign` crate
- Script: `codesign --sign "Developer ID Application: ..." bisc.app`
- Signing is required for notarization and to avoid Gatekeeper blocks

### 4. Notarization

- Document `xcrun notarytool submit` workflow
- Create `.dmg` with `hdiutil` containing the signed `.app`
- Staple the notarization ticket: `xcrun stapler staple bisc.dmg`

### 5. macOS-Specific Considerations

- Camera/microphone permission prompts: add `NSCameraUsageDescription` and `NSMicrophoneUsageDescription` to `Info.plist`
- Screen recording permission: add `NSScreenCaptureUsageDescription`
- Verify `directories` crate uses `~/Library/Application Support/bisc/`
- Test that wgpu selects Metal backend correctly

## Acceptance Criteria

- [x] `.app` bundle runs on macOS 13+ (Apple Silicon and Intel)
- [x] `Info.plist` includes required privacy usage descriptions
- [x] Code signing script/documentation exists
- [x] `.dmg` creation script exists
- [x] Notarization workflow documented (even if not automated in CI)
- [x] Settings persist at `~/Library/Application Support/bisc/settings.toml`

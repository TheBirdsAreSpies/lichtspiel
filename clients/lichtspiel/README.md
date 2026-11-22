# Lichtspiel app (Flutter)

The Lichtspiel app for Windows, Android, macOS, iPhone and iPad. It talks to a Lichtspiel server over the same JSON API as the web app. The interface is in English and German: it follows the device's language, and Settings → Account → Appearance switches it (kept with the account, like in the web app).

## What you need

- Flutter (stable). On a Mac: `brew install --cask flutter`, then `flutter doctor`.
- For macOS and iOS: Xcode from the App Store (the Command Line Tools alone are not enough), then once:
  ```sh
  sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
  sudo xcodebuild -license accept
  sudo xcodebuild -runFirstLaunch
  brew install cocoapods
  ```
  For iOS also install an iOS platform and simulator in Xcode › Settings › Components.
- For Windows: Visual Studio with "Desktop development with C++". For Android: Android Studio.

Ready-made builds (Android, Windows, macOS) are attached to each release on GitHub; the Gitea release has the Android APK. How they are made and signed: [docs/development.md](../../docs/development.md#releases).

Everything below runs in this folder (`clients/lichtspiel`).

```sh
flutter pub get
flutter analyze
flutter test
```

## macOS

```sh
flutter run -d macos             # debug
flutter build macos --release    # build/macos/Build/Products/Release/Lichtspiel.app
```

The first build runs `pod install` in `macos/` (CocoaPods; media_kit has no Swift Package yet, the other plugins come in through Swift Package Manager). The app needs macOS 12 or later, iOS/iPadOS 15 or later.

The app runs in the macOS App Sandbox. Its entitlements (`macos/Runner/*.entitlements`) allow network access (talking to the server and finding it on the home network), reading a picture you pick (poster upload) and writing to the Downloads folder. The sign-in token goes into the login keychain, which needs no extra entitlement.

Signing: for running on your own Mac the automatic "Sign to Run Locally" is enough. To give the app to others, open `macos/Runner.xcworkspace` in Xcode, choose your team under Runner › Signing & Capabilities, and archive (Product › Archive) to notarize it.

## iPhone and iPad

```sh
open -a Simulator
flutter run -d "iPhone 16"                # or any simulator from `flutter devices`
flutter build ios --no-codesign           # checks that it builds, without signing
```

To run on your own device:

1. Open `ios/Runner.xcworkspace` in Xcode (the workspace, not the project, because of CocoaPods).
2. Runner › Signing & Capabilities: pick your team (a free Apple ID works for your own devices, the app then runs for 7 days) and, if needed, change the bundle identifier `de.lichtspiel.lichtspiel` to something unique.
3. Connect the device, turn on Developer Mode on it, and run `flutter run -d <device>` or press Run in Xcode.

`flutter build ipa` makes an App Store / TestFlight build; that needs a paid Apple Developer account.

The first time the app looks for servers iOS asks for access to the local network. If you say no, type the server's address instead (Settings › Privacy › Local Network turns it back on). Downloads go into the app's own folder; the Files app shows them under "On My iPhone › Lichtspiel".

## Signing in

The sign-in screen lists the servers it finds on the home network (the same discovery as Plex: a search on UDP port 32414). Tap one, or type an address such as `192.168.1.20:8484` (`http://` and port 8484 are added when missing). The server address is kept in the app's settings, the sign-in token in the system keychain (Keychain on Apple devices, Keystore on Android, Credential Manager on Windows).

Finding servers on iPhone and iPad needs Apple's multicast entitlement (`com.apple.developer.networking.multicast`), which Apple grants on request. Until the app has it, the search finds nothing there and you type the address; it works on macOS, Windows and Android.

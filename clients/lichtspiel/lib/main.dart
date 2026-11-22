import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:media_kit/media_kit.dart';
import 'api.dart';
import 'i18n.dart';
import 'screens/downloads.dart';
import 'screens/player.dart';
import 'screens/shell.dart';
import 'screens/login.dart';
import 'theme.dart';
import 'widgets/poster.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  MediaKit.ensureInitialized();
  await AppTheme.load();
  await I18n.load();
  runApp(const LichtspielApp());
}

class LichtspielApp extends StatefulWidget {
  const LichtspielApp({super.key});
  @override
  State<LichtspielApp> createState() => _LichtspielAppState();
}

class _LichtspielAppState extends State<LichtspielApp> {
  Api? api;
  bool loading = true;
  // A new language builds every screen again (like the web app reloads), with a new navigator.
  var navKey = GlobalKey<NavigatorState>();

  void _languageChanged() => setState(() => navKey = GlobalKey<NavigatorState>());

  @override
  void dispose() {
    I18n.lang.removeListener(_languageChanged);
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    I18n.lang.addListener(_languageChanged);
    // A token that stops working while the app runs also leads back to the sign-in.
    // (A late answer for an older sign-in is ignored.)
    Api.onUnauthorized = (a) {
      if (a != api) return;
      a.forget();
      _forgetUser();
      navKey.currentState?.popUntil((r) => r.isFirst);
      if (mounted) setState(() => api = null);
    };
    Api.restore().then((a) async {
      // A saved token the server no longer knows sends us back to the sign-in.
      if (a != null) {
        try {
          // Away from home a LAN address may get no answer at all: don't wait for the system to give up.
          final me = await a.get('/api/me').timeout(const Duration(seconds: 8));
          await AppTheme.adopt(me);
          await I18n.adopt(me);
        } on ApiError catch (e) {
          debugPrint('Checking the saved sign-in at ${a.base} failed: ${e.cause ?? e}');
          if (e.status == 401) a = null;
        } catch (e) {
          debugPrint('Checking the saved sign-in at ${a.base} failed: $e');
        }
      }
      if (mounted) setState(() { api = a; loading = false; });
    });
  }

  /// The next person on this device starts clean: no queue, no admin menus, no download rights.
  void _forgetUser() {
    PlayerScreen.userQueue.clear();
    Poster.admin = false;
    DownloadRights.allowed = false;
  }

  Future<void> signOut() async {
    final a = api; // a 401 here signs out already (onUnauthorized) and sets api to null
    if (a == null) return;
    try {
      await a.post('/api/logout');
    } catch (_) {}
    await a.forget();
    _forgetUser();
    if (mounted) setState(() => api = null);
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: AppTheme.changes,
      builder: (context, _) => MaterialApp(
        key: ValueKey(I18n.lang.value),
        navigatorKey: navKey,
        locale: Locale(I18n.lang.value),
        supportedLocales: [for (final c in I18n.names.keys) Locale(c)],
        localizationsDelegates: GlobalMaterialLocalizations.delegates,
        // The mouse's "back" button (button 4) goes one screen back, like in a browser.
        // On a phone held sideways nothing goes under the notch (the strips keep the page colour).
        builder: (context, child) => ColoredBox(
          color: Theme.of(context).scaffoldBackgroundColor,
          child: SafeArea(
            top: false,
            bottom: false,
            child: Listener(
              behavior: HitTestBehavior.translucent,
              onPointerDown: (e) {
                if (e.kind == PointerDeviceKind.mouse && (e.buttons & kBackMouseButton) != 0) navKey.currentState?.maybePop();
              },
              child: child ?? const SizedBox.shrink(),
            ),
          ),
        ),
        title: 'Lichtspiel',
        debugShowCheckedModeBanner: false,
        themeMode: AppTheme.mode,
        theme: AppTheme.light,
        darkTheme: AppTheme.dark,
        home: loading
            ? const Scaffold(body: Center(child: CircularProgressIndicator()))
            : api == null
                ? LoginScreen(onSignedIn: (a) {
                    setState(() => api = a);
                    a.get('/api/me').then((me) async {
                      await AppTheme.adopt(me);
                      await I18n.adopt(me);
                    }).catchError((_) {});
                  })
                : AppShell(api: api!, onSignOut: signOut),
      ),
    );
  }
}

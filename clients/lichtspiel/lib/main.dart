import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:media_kit/media_kit.dart';
import 'api.dart';
import 'screens/shell.dart';
import 'screens/login.dart';
import 'theme.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  MediaKit.ensureInitialized();
  await AppTheme.load();
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
  final navKey = GlobalKey<NavigatorState>();

  @override
  void initState() {
    super.initState();
    // A token that stops working while the app runs also leads back to the sign-in.
    // (A late answer for an older sign-in is ignored.)
    Api.onUnauthorized = (a) {
      if (a != api) return;
      a.forget();
      navKey.currentState?.popUntil((r) => r.isFirst);
      if (mounted) setState(() => api = null);
    };
    Api.restore().then((a) async {
      // A saved token the server no longer knows sends us back to the sign-in.
      if (a != null) {
        try {
          // Away from home a LAN address may get no answer at all: don't wait for the system to give up.
          await AppTheme.adopt(await a.get('/api/me').timeout(const Duration(seconds: 8)));
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

  Future<void> signOut() async {
    final a = api; // a 401 here signs out already (onUnauthorized) and sets api to null
    if (a == null) return;
    try {
      await a.post('/api/logout');
    } catch (_) {}
    await a.forget();
    if (mounted) setState(() => api = null);
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: AppTheme.changes,
      builder: (context, _) => MaterialApp(
        navigatorKey: navKey,
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
                    a.get('/api/me').then(AppTheme.adopt).catchError((_) {});
                  })
                : AppShell(api: api!, onSignOut: signOut),
      ),
    );
  }
}

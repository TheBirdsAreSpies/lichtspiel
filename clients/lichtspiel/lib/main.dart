import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:media_kit/media_kit.dart';
import 'api.dart';
import 'screens/home.dart';
import 'screens/login.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  MediaKit.ensureInitialized();
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
          await a.get('/api/me').timeout(const Duration(seconds: 8));
        } on ApiError catch (e) {
          if (e.status == 401) a = null;
        } catch (_) {}
      }
      if (mounted) setState(() { api = a; loading = false; });
    });
  }

  Future<void> signOut() async {
    try {
      await api!.post('/api/logout');
    } catch (_) {}
    await api!.forget();
    if (mounted) setState(() => api = null);
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      navigatorKey: navKey,
      // The mouse's "back" button (button 4) goes one screen back, like in a browser.
      // On a phone held sideways nothing goes under the notch (the strips stay dark).
      builder: (context, child) => ColoredBox(
        color: const Color(0xFF121212),
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
      theme: ThemeData.dark(useMaterial3: true).copyWith(
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFFE5A00D), brightness: Brightness.dark),
        scaffoldBackgroundColor: const Color(0xFF121212),
      ),
      home: loading
          ? const Scaffold(body: Center(child: CircularProgressIndicator()))
          : api == null
              ? LoginScreen(onSignedIn: (a) => setState(() => api = a))
              : HomeScreen(api: api!, onSignOut: signOut),
    );
  }
}

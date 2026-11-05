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
    Api.restore().then((a) async {
      // A saved token the server no longer knows sends us back to the sign-in.
      if (a != null) {
        try {
          await a.get('/api/me');
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
      builder: (context, child) => Listener(
        behavior: HitTestBehavior.translucent,
        onPointerDown: (e) {
          if (e.kind == PointerDeviceKind.mouse && (e.buttons & kBackMouseButton) != 0) navKey.currentState?.maybePop();
        },
        child: child ?? const SizedBox.shrink(),
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

import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';
import 'link_login.dart';

class LoginScreen extends StatefulWidget {
  final void Function(Api) onSignedIn;
  const LoginScreen({super.key, required this.onSignedIn});
  @override
  State<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends State<LoginScreen> {
  final server = TextEditingController();
  final user = TextEditingController();
  final pass = TextEditingController();
  final code = TextEditingController();
  bool needsCode = false, busy = false;
  String? error;
  List<Map<String, dynamic>> profiles = []; // home profiles (offered in the local network only)
  final serverFocus = FocusNode();

  @override
  void initState() {
    super.initState();
    SharedPreferences.getInstance().then((p) {
      if (mounted) setState(() => server.text = p.getString('server') ?? '');
      loadProfiles();
    });
    serverFocus.addListener(() {
      if (!serverFocus.hasFocus) loadProfiles();
    });
  }

  @override
  void dispose() {
    serverFocus.dispose();
    super.dispose();
  }

  Future<void> loadProfiles() async {
    if (server.text.trim().isEmpty) return;
    try {
      final r = await Api(Api.normalize(server.text)).get('/api/home');
      final list = [for (final x in (r is Map ? r['profiles'] as List? ?? const [] : const [])) Map<String, dynamic>.from(x)];
      if (mounted) setState(() => profiles = list);
    } catch (_) {
      if (mounted) setState(() => profiles = []);
    }
  }

  /// A tap on a profile, and its PIN if it has one.
  Future<void> signInProfile(Map<String, dynamic> p) async {
    var pin = '';
    if (p['has_pin'] == true) {
      final c = TextEditingController();
      final got = await showDialog<String>(
        context: context,
        builder: (d) => AlertDialog(
          title: Text('PIN für ${p['username']}'),
          content: TextField(controller: c, obscureText: true, autofocus: true, keyboardType: TextInputType.number, onSubmitted: (v) => Navigator.pop(d, v)),
          actions: [TextButton(onPressed: () => Navigator.pop(d), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, c.text), child: const Text('Anmelden'))],
        ),
      );
      c.dispose();
      if (got == null) return;
      pin = got;
    }
    setState(() { busy = true; error = null; });
    try {
      final api = Api(Api.normalize(server.text));
      final r = await api.post('/api/home/${p['id']}/login', {'pin': pin, 'device': 'Lichtspiel App'});
      api.token = r['token'];
      await api.save();
      widget.onSignedIn(api);
    } on ApiError catch (e) {
      setState(() => error = e.message);
    } catch (_) {
      setState(() => error = 'Server nicht erreichbar.');
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> signInWithCode() async {
    if (server.text.trim().isEmpty) {
      setState(() => error = 'Bitte zuerst die Server-Adresse eintragen.');
      return;
    }
    setState(() { busy = true; error = null; });
    try {
      final api = Api(Api.normalize(server.text));
      await api.status(); // reachable?
      if (!mounted) return;
      await Navigator.push(context, MaterialPageRoute(builder: (_) => LinkLoginScreen(api: api, onSignedIn: (a) {
        Navigator.of(context).popUntil((r) => r.isFirst);
        widget.onSignedIn(a);
      })));
    } catch (_) {
      setState(() => error = 'Server nicht erreichbar. Adresse prüfen (z. B. 192.168.1.20:8484).');
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> submit() async {
    setState(() { busy = true; error = null; });
    try {
      final api = Api(Api.normalize(server.text));
      await api.status(); // reachable?
      await api.login(user.text, pass.text, code: needsCode ? code.text : null);
      widget.onSignedIn(api);
    } on ApiError catch (e) {
      setState(() { error = e.message; if (e.needsCode) needsCode = true; });
    } catch (_) {
      setState(() => error = 'Server nicht erreichbar. Adresse prüfen (z. B. 192.168.1.20:8484).');
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 380),
          child: AutofillGroup(
            child: ListView(shrinkWrap: true, padding: const EdgeInsets.all(24), children: [
              const Text('Lichtspiel', textAlign: TextAlign.center, style: TextStyle(fontSize: 34, fontWeight: FontWeight.w600)),
              const SizedBox(height: 28),
              TextField(controller: server, focusNode: serverFocus, onSubmitted: (_) => loadProfiles(), decoration: const InputDecoration(labelText: 'Server', hintText: '192.168.1.20:8484')),
              if (profiles.isNotEmpty) ...[
                Wrap(spacing: 10, runSpacing: 10, alignment: WrapAlignment.center, children: [
                  for (final p in profiles)
                    ActionChip(
                      avatar: CircleAvatar(child: Text('${p['username']}'.isEmpty ? '?' : '${p['username']}'[0].toUpperCase())),
                      label: Text('${p['username']}${p['has_pin'] == true ? ' 🔒' : ''}'),
                      onPressed: busy ? null : () => signInProfile(p),
                    ),
                ]),
                const Padding(padding: EdgeInsets.symmetric(vertical: 8), child: Center(child: Text('oder mit Passwort anmelden', style: TextStyle(color: Colors.white54)))),
              ],
              TextField(controller: user, autofillHints: const [AutofillHints.username], decoration: const InputDecoration(labelText: 'Benutzername')),
              TextField(controller: pass, obscureText: true, autofillHints: const [AutofillHints.password], onSubmitted: (_) => submit(), decoration: const InputDecoration(labelText: 'Passwort')),
              if (needsCode)
                TextField(controller: code, keyboardType: TextInputType.number, onSubmitted: (_) => submit(), decoration: const InputDecoration(labelText: 'Code aus der Authenticator-App')),
              if (error != null)
                Padding(padding: const EdgeInsets.only(top: 12), child: Text(error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
              const SizedBox(height: 20),
              FilledButton(
                onPressed: busy ? null : submit,
                child: busy ? const SizedBox(height: 18, width: 18, child: CircularProgressIndicator(strokeWidth: 2)) : const Text('Anmelden'),
              ),
              const SizedBox(height: 10),
              OutlinedButton(onPressed: busy ? null : signInWithCode, child: const Text('Mit Code anmelden')),
            ]),
          ),
        ),
      ),
    );
  }
}

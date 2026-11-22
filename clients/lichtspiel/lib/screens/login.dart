import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';
import '../discovery.dart';
import 'link_login.dart';
import '../theme.dart';
import '../i18n.dart';

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
  List<FoundServer> found = []; // servers that answered on the home network
  bool searching = false;

  @override
  void initState() {
    super.initState();
    SharedPreferences.getInstance().then((p) {
      if (mounted) setState(() => server.text = p.getString('server') ?? '');
      loadProfiles();
    });
    search();
    serverFocus.addListener(() {
      if (!serverFocus.hasFocus) loadProfiles();
    });
  }

  @override
  void dispose() {
    serverFocus.dispose();
    super.dispose();
  }

  /// Looks for servers on the home network; quietly finds nothing when the network doesn't allow it.
  Future<void> search() async {
    setState(() => searching = true);
    final list = await discoverServers();
    if (!mounted) return;
    setState(() { found = list; searching = false; });
    // Only one server and nothing typed yet: take it.
    if (list.length == 1 && server.text.trim().isEmpty) pick(list.first);
  }

  void pick(FoundServer s) {
    setState(() => server.text = s.address);
    loadProfiles();
  }

  Future<void> loadProfiles() async {
    if (server.text.trim().isEmpty) return;
    try {
      final r = await Api(Api.normalize(server.text)).get('/api/home');
      final list = [for (final x in (r is Map ? r['profiles'] as List? ?? const [] : const [])) Map<String, dynamic>.from(x)];
      if (mounted) setState(() => profiles = list);
    } catch (e) {
      debugPrint('Loading profiles from ${Api.normalize(server.text)} failed: $e');
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
          title: Text(tr('PIN for {p}', {'p': p['display_name'] ?? p['username']})),
          content: TextField(controller: c, obscureText: true, autofocus: true, keyboardType: TextInputType.number, onSubmitted: (v) => Navigator.pop(d, v)),
          actions: [TextButton(onPressed: () => Navigator.pop(d), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, c.text), child: Text(tr('Sign in')))],
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
      debugPrint('Profile sign-in to ${Api.normalize(server.text)} failed: ${e.cause ?? e}');
      setState(() => error = e.message);
    } catch (e) {
      debugPrint('Profile sign-in to ${Api.normalize(server.text)} failed: $e');
      setState(() => error = tr('Server not reachable.'));
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> signInWithCode() async {
    if (server.text.trim().isEmpty) {
      setState(() => error = tr("Please enter the server's address first."));
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
      setState(() => error = tr('Server not reachable. Check the address (e.g. 192.168.1.20:8484).'));
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> submit() async {
    setState(() { busy = true; error = null; });
    try {
      final api = await Api.reach(server.text); // reachable? (an https address may end up on http)
      await api.login(user.text, pass.text, code: needsCode ? code.text : null);
      if (api.base != Api.normalize(server.text) && mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('The server does not speak HTTPS – the connection runs unencrypted over http://'))));
      }
      widget.onSignedIn(api);
    } on ApiError catch (e) {
      debugPrint('Sign-in to ${Api.normalize(server.text)} failed: ${e.cause ?? e}');
      setState(() { error = e.status == 0 ? tr('{message}\nCheck the address (e.g. 192.168.1.20:8484)', {'message': e.message}) : e.message; if (e.needsCode) needsCode = true; });
    } catch (e) {
      debugPrint('Sign-in to ${Api.normalize(server.text)} failed: $e');
      setState(() => error = tr('Server not reachable. Check the address (e.g. 192.168.1.20:8484).'));
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 380),
          child: AutofillGroup(
            child: ListView(shrinkWrap: true, padding: const EdgeInsets.all(24), children: [
              const Text('Lichtspiel', textAlign: TextAlign.center, style: TextStyle(fontSize: 34, fontWeight: FontWeight.w600)),
              const SizedBox(height: 28),
              if (found.isNotEmpty) ...[
                Text(tr('Found on the network'), style: TextStyle(color: context.faint)),
                for (final s in found)
                  ListTile(
                    contentPadding: EdgeInsets.zero,
                    leading: const Icon(Icons.dns_outlined),
                    title: Text(s.name),
                    subtitle: Text(s.address.replaceFirst(RegExp(r'^https?://'), '')),
                    trailing: Api.normalize(server.text) == s.address ? const Icon(Icons.check) : null,
                    onTap: busy ? null : () => pick(s),
                  ),
              ],
              TextField(
                controller: server,
                focusNode: serverFocus,
                keyboardType: TextInputType.url,
                autocorrect: false,
                onSubmitted: (_) => loadProfiles(),
                decoration: InputDecoration(
                  labelText: found.isEmpty ? tr('Server') : tr('Or type an address'),
                  hintText: '192.168.1.20:8484',
                  suffixIcon: searching
                      ? const Padding(padding: EdgeInsets.all(14), child: SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)))
                      : IconButton(tooltip: tr('Search the network'), icon: const Icon(Icons.wifi_find), onPressed: search),
                ),
              ),
              if (profiles.isNotEmpty) ...[
                Wrap(spacing: 10, runSpacing: 10, alignment: WrapAlignment.center, children: [
                  for (final p in profiles)
                    ActionChip(
                      avatar: CircleAvatar(child: Text('${p['display_name'] ?? p['username']}'.isEmpty ? '?' : '${p['display_name'] ?? p['username']}'[0].toUpperCase())),
                      label: Text('${p['display_name'] ?? p['username']}${p['has_pin'] == true ? ' 🔒' : ''}'),
                      onPressed: busy ? null : () => signInProfile(p),
                    ),
                ]),
                Padding(padding: EdgeInsets.symmetric(vertical: 8), child: Center(child: Text(tr('or sign in with a password'), style: TextStyle(color: context.faint)))),
              ],
              TextField(controller: user, autofillHints: const [AutofillHints.username], decoration: InputDecoration(labelText: tr('User name'))),
              TextField(controller: pass, obscureText: true, autofillHints: const [AutofillHints.password], onSubmitted: (_) => submit(), decoration: InputDecoration(labelText: tr('Password'))),
              if (needsCode)
                TextField(controller: code, keyboardType: TextInputType.number, onSubmitted: (_) => submit(), decoration: InputDecoration(labelText: tr('Code from the authenticator app'))),
              if (error != null)
                Padding(padding: const EdgeInsets.only(top: 12), child: Text(error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
              const SizedBox(height: 20),
              FilledButton(
                onPressed: busy ? null : submit,
                child: busy ? const SizedBox(height: 18, width: 18, child: CircularProgressIndicator(strokeWidth: 2)) : Text(tr('Sign in')),
              ),
              const SizedBox(height: 10),
              OutlinedButton(onPressed: busy ? null : signInWithCode, child: Text(tr('Sign in with a code'))),
            ]),
          ),
        ),
      )),
    );
  }
}

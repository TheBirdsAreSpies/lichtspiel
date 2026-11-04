import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';

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

  @override
  void initState() {
    super.initState();
    SharedPreferences.getInstance().then((p) {
      if (mounted) setState(() => server.text = p.getString('server') ?? '');
    });
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
              TextField(controller: server, decoration: const InputDecoration(labelText: 'Server', hintText: '192.168.1.20:8484')),
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
            ]),
          ),
        ),
      ),
    );
  }
}

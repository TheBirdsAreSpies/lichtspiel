import 'dart:async';
import 'package:flutter/material.dart';
import 'package:qr_flutter/qr_flutter.dart';
import '../api.dart';
import '../theme.dart';

/// Sign in with a short code shown here and typed on another device.
class LinkLoginScreen extends StatefulWidget {
  final Api api;
  final void Function(Api) onSignedIn;
  const LinkLoginScreen({super.key, required this.api, required this.onSignedIn});
  @override
  State<LinkLoginScreen> createState() => _LinkLoginScreenState();
}

class _LinkLoginScreenState extends State<LinkLoginScreen> {
  String? code, id, error;
  Timer? poll;
  bool done = false;

  @override
  void initState() {
    super.initState();
    start();
  }

  @override
  void dispose() {
    poll?.cancel();
    super.dispose();
  }

  Future<void> start() async {
    poll?.cancel();
    setState(() { code = null; error = null; });
    try {
      final r = await widget.api.post('/api/link', {'device': 'Lichtspiel App'});
      if (!mounted) return;
      setState(() { id = r['id']; code = r['code']; });
      poll = Timer.periodic(const Duration(seconds: 2), (_) => check());
    } on ApiError catch (e) {
      if (mounted) setState(() => error = e.message);
    } catch (_) {
      if (mounted) setState(() => error = 'Server nicht erreichbar.');
    }
  }

  Future<void> check() async {
    if (done || id == null) return;
    try {
      final r = await widget.api.get('/api/link/$id');
      if (r is Map && r['status'] == 'ok' && !done) {
        done = true;
        poll?.cancel();
        widget.api.token = r['token'];
        await widget.api.save();
        widget.onSignedIn(widget.api);
      }
    } on ApiError catch (e) {
      if (e.status == 410) start(); // the code ran out: show a fresh one
    } catch (_) {/* network hiccup: try again on the next tick */}
  }

  @override
  Widget build(BuildContext context) {
    final host = widget.api.base.replaceFirst(RegExp(r'^https?://'), '');
    // Scanning opens the web page with the code already typed in.
    final link = '${widget.api.base.replaceFirst(RegExp(r'/+$'), '')}/#/link/${code ?? ''}';
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 520),
          child: ListView(shrinkWrap: true, padding: const EdgeInsets.all(24), children: [
            const Text('Mit Code anmelden', textAlign: TextAlign.center, style: TextStyle(fontSize: 28, fontWeight: FontWeight.w600)),
            const SizedBox(height: 20),
            Text('Scanne den QR-Code mit einem Handy, das schon angemeldet ist. Oder öffne dort im Browser\nhttp://$host/#/link\nund gib diesen Code ein:',
                textAlign: TextAlign.center, style: TextStyle(color: context.soft, height: 1.5)),
            const SizedBox(height: 24),
            if (error != null)
              Text(error!, textAlign: TextAlign.center, style: TextStyle(color: Theme.of(context).colorScheme.error))
            else if (code == null)
              const Center(child: CircularProgressIndicator())
            else
              Text(code!, textAlign: TextAlign.center, style: const TextStyle(fontSize: 72, fontWeight: FontWeight.w700, letterSpacing: 14)),
            if (error == null && code != null) ...[
              const SizedBox(height: 24),
              Center(child: Container(
                color: Colors.white,
                padding: const EdgeInsets.all(10),
                child: QrImageView(data: link, size: 180, backgroundColor: Colors.white),
              )),
            ],
            const SizedBox(height: 28),
            if (error != null) FilledButton(onPressed: start, child: const Text('Erneut versuchen')),
            if (error != null) const SizedBox(height: 10),
            OutlinedButton(autofocus: error == null, onPressed: () => Navigator.pop(context), child: const Text('Zurück')),
          ]),
        ),
      ),
    );
  }
}

import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/api.dart';

void main() {
  test('server address gets scheme and default port', () {
    expect(Api.normalize('nas'), 'http://nas:8484');
    expect(Api.normalize('192.168.1.20:9000/'), 'http://192.168.1.20:9000');
    expect(Api.normalize('https://nas.example.com:8484'), 'https://nas.example.com:8484');
  });
}

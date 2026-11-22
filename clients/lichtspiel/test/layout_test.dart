import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:lichtspiel/screens/detail.dart';
import 'package:lichtspiel/screens/library.dart';
import 'package:lichtspiel/screens/shell.dart';
import 'package:lichtspiel/screens/stats.dart';
import 'package:lichtspiel/screens/watchlist.dart';

/// The main screens are drawn on a phone and on a computer without overflowing or throwing.

const _movie = {
  'ratingKey': '1',
  'id': 1,
  'type': 'movie',
  'title': 'Ein sehr langer Filmtitel, der auf einem Telefon umbrechen muss',
  'year': 2021,
  'duration': 7260000,
  'viewOffset': 1200000,
  'contentRating': 'FSK 12',
  'summary': 'Eine Handlung, die über mehrere Zeilen geht und auf dem Telefon die ganze Breite bekommt.',
  'genres': ['Drama', 'Thriller', 'Krimi'],
  'studio': 'Ein Studio',
  'thumb': '/library/metadata/1/thumb',
  'extra': {
    'metadata_source': 'tmdb',
    'ratings': {
      'imdb': {'score': '7.9/10', 'votes': 123456},
      'rotten_tomatoes': {'score': '91%', 'count': 210, 'count_label': 'reviews'},
      'metacritic': {'score': '78'},
    },
    'directors': ['Jemand Bekanntes'],
    'ids': {'imdb': 'tt1', 'tmdb': 2},
    'cast': [
      {'name': 'Erste Person', 'role': 'Hauptrolle'},
      {'name': 'Zweite Person', 'role': 'Nebenrolle'},
    ],
  },
  'Media': [
    {'container': 'matroska', 'size': 4200000000, 'videoCodec': 'hevc', 'width': 3840, 'height': 2160, 'bitrate': 18000, 'audioCodec': 'eac3', 'audioChannels': 6, 'file': '/volume1/video/Filme/Ein sehr langer Filmtitel (2021)/Ein sehr langer Filmtitel (2021).mkv'},
  ],
};

const _season = {'ratingKey': '10', 'id': 10, 'type': 'season', 'title': 'Staffel 1', 'parentTitle': 'Eine Serie', 'parentRatingKey': '9', 'leafCount': 3, 'viewedLeafCount': 1, 'thumb': '/x'};

List<Map<String, dynamic>> _episodes() => [
      for (var i = 1; i <= 3; i++)
        {
          'ratingKey': '${100 + i}',
          'id': 100 + i,
          'type': 'episode',
          'index': i,
          'parentIndex': 1,
          'title': 'Folge $i mit einem längeren Namen',
          'duration': 2700000,
          'viewCount': i == 1 ? 1 : 0,
          'viewOffset': i == 2 ? 900000 : 0,
          'missing': i == 3,
          'summary': 'Was in dieser Folge passiert.',
        },
    ];

/// A year of viewing: an evening episode most days, a movie at the weekend.
Map<String, dynamic> _stats() {
  final now = DateTime.now();
  final plays = <List<Object>>[];
  for (var d = 300; d >= 0; d--) {
    final day = DateTime(now.year, now.month, now.day - d, 20 + d % 3);
    plays.add([day.millisecondsSinceEpoch ~/ 1000, 1, day.weekday >= 6 ? 'movie' : 'episode', day.weekday >= 6 ? 110.5 : 42.0]);
  }
  final title = {'id': 1, 'title': 'Ein sehr langer Filmtitel, der auf einem Telefon umbrechen muss', 'year': 2021, 'thumb': '/library/metadata/1/thumb', 'plays': 12, 'minutes': 1300.5};
  return {'days': 365, 'from': now.millisecondsSinceEpoch ~/ 1000 - 365 * 86400, 'size': plays.length, 'minutes': 15000.0, 'plays': plays,
    'shows': [{...title, 'type': 'show'}], 'movies': [{...title, 'type': 'movie', 'id': 2}], 'genres': [{'genre': 'Drama', 'plays': 200, 'minutes': 9000.0}, {'genre': 'Komödie', 'plays': 50, 'minutes': 2000.0}],
    'users': [{'id': 1, 'username': 'anna', 'display_name': 'Anna mit einem langen Namen', 'plays': 301, 'minutes': 15000.0}]};
}

http.Response _json(Object body) => http.Response.bytes(utf8.encode(jsonEncode(body)), 200, headers: {'content-type': 'application/json'});

Future<http.Response> _server(http.Request r) async {
  final p = r.url.path;
  if (p == '/api/me') return _json({'admin': true, 'can_download': true});
  if (p == '/library/metadata/1') return _json({'MediaContainer': {'Metadata': [_movie]}});
  if (p == '/library/metadata/10') return _json({'MediaContainer': {'Metadata': [_season]}});
  if (p == '/library/metadata/9') return _json({'MediaContainer': {'Metadata': [{'ratingKey': '9', 'type': 'show', 'title': 'Eine Serie'}]}});
  if (p == '/library/metadata/10/children') return _json({'MediaContainer': {'Metadata': _episodes()}});
  if (p == '/library/sections') return _json({'MediaContainer': {'Directory': [{'key': '1', 'type': 'movie', 'title': 'Filme', 'count': 12}, {'key': '2', 'type': 'music', 'title': 'Musik', 'count': 3}]}});
  if (p == '/hubs') return _json({'MediaContainer': {'Hub': [{'title': 'Weiterschauen', 'hubIdentifier': 'continueWatching', 'Metadata': [_movie]}]}});
  if (p.startsWith('/library/sections/1/all')) return _json({'MediaContainer': {'totalSize': 2, 'Metadata': [_movie, {..._movie, 'ratingKey': '2', 'id': 2}]}});
  if (p == '/api/stats/plays') return _json(_stats());
  if (p == '/api/users') return _json({'users': [{'id': 1, 'username': 'anna', 'display_name': 'Anna mit einem langen Namen'}, {'id': 2, 'username': 'ben'}]});
  if (p == '/api/dashboard/stats') {
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    return _json({'samples': [for (var i = 0; i < 48; i++) {'at': now - i * 3600, 'cpu': 5 + i % 24, 'streams': i % 3, 'transcodes': i % 2}]});
  }
  if (p.startsWith('/api/watchlist')) return _json({'MediaContainer': {'Metadata': [_movie]}});
  return _json({});
}

Future<void> _show(WidgetTester tester, Size size, Widget screen) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await http.runWithClient(() async {
    await tester.pumpWidget(MaterialApp(theme: ThemeData.dark(useMaterial3: true), home: screen));
    for (var i = 0; i < 10; i++) {
      await tester.pump(const Duration(milliseconds: 50));
    }
  }, () => MockClient(_server));
  expect(tester.takeException(), isNull);
}

void main() {
  final api = Api('http://nas:8484', 't');
  const phone = Size(390, 844), computer = Size(1280, 800);

  for (final (name, size) in [('phone', phone), ('computer', computer)]) {
    testWidgets('a movie page on a $name', (tester) async {
      await _show(tester, size, DetailScreen(api: api, id: '1'));
      expect(find.textContaining('Ein sehr langer Filmtitel'), findsWidgets);
      expect(find.textContaining('Fortsetzen bei'), findsOneWidget);
    });

    testWidgets('a season with its episodes on a $name', (tester) async {
      await _show(tester, size, DetailScreen(api: api, id: '10'));
      expect(find.text('1. Folge 1 mit einem längeren Namen'), findsOneWidget);
      expect(find.textContaining('Datei fehlt'), findsOneWidget);
    });

    testWidgets('a library on a $name', (tester) async {
      await _show(tester, size, LibraryScreen(api: api, section: const {'key': '1', 'type': 'show', 'title': 'Serien'}));
      expect(find.byType(SegmentedButton<String>), findsOneWidget);
    });

    testWidgets('the statistics on a $name', (tester) async {
      await _show(tester, size, StatsScreen(api: api, admin: true));
      expect(find.text('Lieblingszeit'), findsOneWidget);
      // Down to the end: every part of the page is drawn once without overflowing.
      for (final part in ['Tageszeiten', 'Meistgeschaute Serien', 'Genres', 'Last auf dem Server nach Tageszeit']) {
        await tester.scrollUntilVisible(find.text(part), 300, scrollable: find.byType(Scrollable).first);
        expect(tester.takeException(), isNull);
      }
    });

    testWidgets('the watchlist on a $name', (tester) async {
      await _show(tester, size, WatchlistScreen(api: api));
    });

    testWidgets('the main screens on a $name', (tester) async {
      await _show(tester, size, AppShell(api: api, onSignOut: () {}));
      expect(find.text('Bibliotheken'), findsOneWidget);
      await http.runWithClient(() async {
        await tester.tap(find.text('Bibliotheken'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 50));
      }, () => MockClient(_server));
      expect(find.text('Musik'), findsOneWidget);
      expect(find.byIcon(Icons.library_music_outlined), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}

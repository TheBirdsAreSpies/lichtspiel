# The API

Every feature of the web app is an HTTP + JSON call. The full list is built in: open `http://<server>:8484/api/docs` or the **API** page in the web app. Back to the [README](../README.md).

Sign in once, then send the token with every call:

```
POST /api/login            {"username":"me","password":"…"}   → {"token":"…"}
GET  /library/sections     X-Token: <token>
GET  /library/sections/1/all?type=movie&genre=Drama&year>=2000&unwatched=1&sort=rating:desc
GET  /library/metadata/42/children
GET  /video/transcode/42/stream.mp4?quality=720p-4&offset=600
POST /playlists            {"title":"Short comedies","kind":"smart",
                            "query":{"match_all":true,"rules":[
                              {"field":"genre","op":"is","value":"Comedy"},
                              {"field":"duration","op":"lt","value":95}],
                             "sort":"year:desc","limit":50}}
GET  /playlists?kind=smart&sort=updated_at:desc
POST /api/invites          {"email":"friend@example.com","days":7,"allowed_sections":[1]}
POST /api/downloads        {"ids":[42],"quality":"720p-4"}   → then GET /api/downloads/{id}/file
```

The token can also go in `Authorization: Bearer <token>`, but not in the address: `?token=` is
not accepted. A link that has to work on its own (a video for a player, a download) gets a
`?ticket=` from `GET /api/ticket`. Lists come
back as `{"MediaContainer": {"size": …, "Metadata": [ … ]}}`. `GET /api/filters` lists every
field, operator and sort key; `GET /api/docs` lists every call.


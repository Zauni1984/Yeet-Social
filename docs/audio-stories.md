# Audio Stories

Sprachnachrichten als Posts: direkt im Browser aufnehmen (Smartphone wie
Desktop), nach 24 Stunden weg oder permanent, optional 18+.

## Nutzerseite

- Eigener Tab **AUDIO** im Feed. Oben eine Karte „Audio Stories → Aufnehmen".
- Aufnahme-Dialog: großer Aufnahmeknopf, Timer (max. 180 s, stoppt
  automatisch), Pegelanzeige, danach Vorschau-Player, optionaler Text
  (≤ 420 Zeichen), Wahl **Verschwindet in 24 h** / **Permanent**, Häkchen
  **18+** (nur sichtbar, wenn 18+-Inhalte in den Einstellungen aktiv sind),
  **Veröffentlichen**.
- Browser ohne `MediaRecorder` (sehr alte iOS-Versionen): Knopf deaktiviert,
  stattdessen „Audiodatei wählen" mit `capture` → öffnet auf dem Handy die
  native Aufnahme-App.
- Karten zeigen einen Player mit Mikrofon-Icon und Label „Audio Story";
  Pay-per-View funktioniert wie bei Videos (gesperrte Kachel).
- **Vorleser** (Barrierefreiheit): bei einer Audio Story wird Autor + „Audio-
  Post" + Text angesagt, dann die Aufnahme abgespielt, danach geht es zum
  nächsten Post; Pause/Stopp wirken auch auf die Aufnahme.
- 18+-Audio-Stories folgen den bestehenden Regeln: nicht im AUDIO-Tab, sondern
  im 18+-Tab (mit Player).
- Permanente Audio Stories bleiben im AUDIO-Tab dauerhaft sichtbar (er ist ihr
  Zuhause); im globalen Feed gilt wie bei Textposts die 24-h-Sicht.

## Technik

| Teil | Ort |
| --- | --- |
| Recorder-Modul `YeetAudio` | `frontend/index.html` (Script-Block am Ende) |
| Karten-Rendering | `renderMediaUrl()` (`kind === 'audio'` oder Audio-Endung) |
| Upload | `POST /api/v1/uploads/post-media` — akzeptiert jetzt `audio/webm→.weba`, `audio/mp4|x-m4a|aac→.m4a`, `audio/mpeg→.mp3`, `audio/ogg|opus→.ogg`, `audio/wav→.wav`, max. 16 MB; MIME-Parameter (`;codecs=opus`) werden ignoriert |
| Post | `POST /api/v1/posts` mit `kind:"audio"`, `media_url` Pflicht, `content` darf leer sein |
| Feed | `GET /api/v1/feed?kind=audio` (permanente Posts ohne 24-h-Grenze) |
| Migration | `0046_post_kind_audio.sql` (`posts.kind`, Default `text`) |

Aufnahmeformat: `audio/webm;codecs=opus` (Chrome/Firefox/Android), `audio/mp4`
(Safari/iOS); die Endung im Upload richtet sich nach dem MIME-Typ, damit der
Client Audio (`.weba`) und Video (`.webm`) an der URL unterscheiden kann.

Punkte-Belohnung: wie bei allen Posts nur ab 120 Zeichen Text — eine reine
Sprachnachricht ohne Text erhält aktuell keine Posting-Punkte (bewusst offen
gelassen, bis wir eine Regel für Audio festlegen).

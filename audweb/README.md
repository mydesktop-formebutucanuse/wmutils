# audweb

Plays YouTube, SoundCloud and the other sites
[yt-dlp](https://github.com/yt-dlp/yt-dlp) knows in Audacious, and takes
over what LibreWolf plays: a button in LibreWolf, and the track goes on in
Audacious from where it was.

| File | What it is |
|---|---|
| [audweb](audweb) | The command |
| [audweb-host](audweb-host) | Runs audweb for the LibreWolf extension (native messaging) |
| [audweb.json.in](audweb.json.in) | The native messaging manifest, with the host's path to fill in |
| [librewolf](librewolf) | The LibreWolf (Firefox) extension |
| [install.sh](install.sh) | Installs all of it for one user, without root |

On Gentoo the package installs `audweb` and `audweb-host` in `/usr/bin`,
the manifest in `/usr/lib64/mozilla/native-messaging-hosts` and the
extension in LibreWolf's own extensions directory (see the
[top README](../README.md) and [From LibreWolf](#from-librewolf)). By
hand, `sh audweb/install.sh` puts the commands in `~/.local/bin`, the
manifest in `~/.mozilla/native-messaging-hosts` and the extension in
`~/.local/share/audweb/audweb.xpi`; `sh audweb/install.sh remove` takes
them out again. The extension is then added to LibreWolf as
[From LibreWolf](#from-librewolf) says.

```sh
audweb 'https://www.youtube.com/watch?v=...'     # play it now
audweb 'https://soundcloud.com/artist/sets/...'  # a whole set
audweb -e 'https://youtu.be/...' 'https://soundcloud.com/...'   # add to the playlist
audweb -t 95 'https://www.youtube.com/watch?v=...'  # from 1:35
audweb -e -n 20 -a 'https://soundcloud.com/a/track' 'https://soundcloud.com/a/sets/set'
                                                   # the 20 after the track in the set
```

Without `-e` the tracks go into Audacious's "Now Playing" playlist, which is
emptied first, and the first one starts. With `-e` they are added to the end
of the current playlist. A URL can be one track or a playlist, set, album or
channel; each track goes into Audacious as soon as yt-dlp has found it, so
the first plays while the others are still being looked up. Audacious is
started if it is not running.

`-t SECONDS` starts the first track that far in. With `-c` as well, the
seconds are where the track was when audweb was started somewhere else,
where it goes on playing: audweb adds the time it takes until Audacious
plays, about 5 s, so that both are at the same place then. The extension
uses this.

`-a TRACK` (with `-e`) adds, of each URL's list, what comes after TRACK
(all of it when TRACK is not in it), and never a track twice, nor TRACK
itself. `-n N` adds N tracks at most. The extension uses these for what the
site would have played after the track it handed over.

## From LibreWolf

On a YouTube or SoundCloud page, the extension's toolbar button, or
Alt+Shift+A, hands what plays over to Audacious: the tab plays on while
audweb starts, and is paused once Audacious plays the same track from the
same place, so there is no gap. On SoundCloud it is the track in the player
bar at the bottom, whatever page is open; on YouTube, the video, at its
time in the video also while an ad plays. On other pages it is the page, at
the time of its playing video or audio element, if yt-dlp knows the site. A
paused tab is taken over too, from where it stopped.

What the site would have played after it follows in Audacious, added
behind it while it plays, up to 50 tracks:

| | After the track |
|---|---|
| SoundCloud | The "Next up" queue, in its order (the extension opens the queue for a moment to read it, if it is closed); then the rest of the set the track plays from, for what a long queue had not shown; then, if autoplay is on, its station: tracks related to the last one in the queue |
| YouTube, in a playlist or Mix | The rest of the list, from after the video |
| YouTube, no list, autoplay on | The video autoplay has chosen, then YouTube's Mix for the video, which goes on as autoplay does |
| YouTube, no list, autoplay off | Nothing, as in the browser |

Autoplay is read from its switch in the player; a player without the switch
there (YouTube does not serve the same one to everybody) counts as autoplay
on.

They are looked up and downloaded one after the other, a few seconds each,
so a long list fills in over a few minutes, well before the first track
ends; 50 tracks play for about 3 hours. Handing over again stops what an
earlier hand-over was still adding. Messages from adding them go to
`~/.cache/audweb/next.log`, and what the extension sent last to
`~/.cache/audweb/host.log`.

While audweb starts the button shows "…"; if something goes wrong it shows
"!", and its tooltip says what (yt-dlp's error, or that audweb-host could
not be started). After a hand-over the tooltip says whether what follows is
being added, or why nothing follows.

The extension is not signed by Mozilla, which LibreWolf, like Firefox,
wants of an extension that a user adds. One in LibreWolf's own directory,
`/usr/lib64/librewolf/browser/extensions`, need not be: LibreWolf is built
to take unsigned extensions from there, and only there, so signing stays
required for every other one. The Gentoo package puts it there; by hand,
as root, after `sh audweb/install.sh`:

```sh
install -Dm644 ~/.local/share/audweb/audweb.xpi \
	/usr/lib64/librewolf/browser/extensions/audweb@wmutils.xpi
```

The file must be named after the extension's id, `audweb@wmutils.xpi`.
LibreWolf finds it at its next start, for every profile, and it is on; it
is listed in `about:addons`, where the shortcut can be changed too (the
gear menu, "Manage Extension Shortcuts"). After a new version is copied
there, LibreWolf takes it at its next start. To take it out, remove the
file.

Without root there is `about:debugging#/runtime/this-firefox`, "Load
Temporary Add-on...", and `audweb.xpi` or `librewolf/manifest.json`: until
LibreWolf is closed.

LibreWolf started through [nsrun](../nsrun) cannot use it: the extension
starts audweb-host inside the compartment, which has neither the real home
nor the session's D-Bus that audtool talks to Audacious through.

## How the tracks get to Audacious

**YouTube** serves the sound as one plain HTTPS file. yt-dlp finds its
address (Opus if there is one, else AAC), and audweb downloads it with curl
into `~/.cache/audweb` and Audacious plays it from there, with the title,
artist and length from yt-dlp and the site's name as the album, through a
one-track XSPF playlist in `~/.cache/audweb/xspf`. audweb asks for the file
4 MB at a time: YouTube sends the address alone slowly once the file is
over 10 MB or so (30 KB/s for a 15 MB one), so Audacious reading it from
the site took half a minute to start a long track, and could fail moving
on to it, while each range comes at full speed (15 MB in about 3 s). A new
address also answers 403 for a second or two, which audweb waits out. Sound
starts 5 to 8 s after the command, most of it yt-dlp finding the address.

Other sites that serve one plain HTTPS file whose size yt-dlp does not
know are streamed: Audacious reads the file from the site itself. Such an
address works for some hours only: a track left in a playlist overnight no
longer plays; give audweb the page again.

**SoundCloud** serves its tracks only as HLS, many small pieces with a list
of them, which Audacious cannot read. Those tracks are downloaded first by
yt-dlp, with their title and artist and "SoundCloud" as the album, into
`~/.cache/audweb`, and played from there. A track takes from about 3 s to
30 s, as SoundCloud allows; in a set the first plays once it is there and
the others are added one by one while it plays.

A downloaded track is played again without downloading it again; one not
played for 7 days is removed. A track is 1 MB or so a minute.

Neither way goes through the site's player, so there are no ads before or
between the tracks: YouTube's ads are separate videos that its player puts
in, not part of the sound file. A sponsor message read out in the video
itself is in the sound, and stays.

## Needs

- `net-misc/yt-dlp`, and `net-misc/curl` for YouTube's downloads. The sites
  change; when a site stops working, a newer yt-dlp usually fixes it.
- `media-sound/audacious` and `media-plugins/audacious-plugins` with the
  `http` USE flag, for streaming over HTTPS from other sites, and `ffmpeg`, for YouTube's
  Opus in WebM and AAC in fragmented MP4 and SoundCloud's M4A. `opus` is
  not needed for these but does no harm. In
  `/etc/portage/package.use/audacious`:

  ```
  media-plugins/audacious-plugins http ffmpeg opus
  ```

  On arm64 both Audacious packages are `~arm64`, in
  `/etc/portage/package.accept_keywords/audacious`:

  ```
  media-sound/audacious ~arm64
  media-plugins/audacious-plugins ~arm64
  ```

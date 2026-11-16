// SPDX-License-Identifier: GPL-3.0-or-later
// The toolbar button and Alt+Shift+A: the track this tab plays goes on in
// Audacious, at the same place. The tab plays on until Audacious plays
// there (audweb-host answers then), and is paused after that.
"use strict";

// Runs in the page. Says what plays and where, and where what the site
// plays after it comes from: {url, time, playing, next: [URL...]}, or
// {error}. On its own, since only the function's text goes to the page.
async function readPage() {
  const host = location.hostname;
  const site = (name) => host === name || host.endsWith("." + name);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  if (site("soundcloud.com")) {
    // SoundCloud's <audio> is not in the page; the player bar at the
    // bottom has the track and the time, whatever page is open.
    const link = document.querySelector(".playbackSoundBadge__titleLink");
    if (!link) return { error: "SoundCloud is not playing anything" };
    const url = new URL(link.getAttribute("href"), location.origin);
    url.search = ""; // ?in=SET: the track alone, not the set
    url.hash = "";
    let time = NaN;
    const bar = document.querySelector(".playbackTimeline__progressWrapper[aria-valuenow]");
    if (bar) time = Number(bar.getAttribute("aria-valuenow"));
    if (!Number.isFinite(time)) {
      const text = document.querySelector(".playbackTimeline__timePassed span[aria-hidden]")?.textContent ?? "";
      time = text.trim().split(":").reduce((t, x) => t * 60 + Number(x), 0);
    }
    const playing = !!document.querySelector(".playControl.playing");

    // What comes after: the "Next up" queue, as SoundCloud would play it.
    // Its tracks are in the page only while it is open, so it is opened
    // for a moment if it is not.
    const queued = [];
    const track = (a) => {
      const u = new URL(a.getAttribute("href"), location.origin);
      u.search = "";
      u.hash = "";
      return u.href;
    };
    const queue = document.querySelector(".queue");
    const opener = document.querySelector(".playbackSoundBadge__showQueue");
    const opened = queue && opener && !queue.classList.contains("m-visible");
    if (opened) {
      opener.click();
      for (let i = 0; i < 20 && !document.querySelector(".queue .queueItemView"); i++) await sleep(100);
    }
    const views = [...document.querySelectorAll(".queue .queueItemView")];
    const active = views.findIndex((v) => v.classList.contains("m-active"));
    for (const v of views.slice(active + 1)) {
      const a = v.querySelector(".queueItemView__title a[href]");
      if (a && v.classList.contains("m-upcoming")) queued.push(track(a));
    }
    const autoplay = !!document.querySelector(".queueFallback__toggle .sc-toggle-on");
    if (opened) document.querySelector(".queue__hide")?.click();
    // The set it plays from, for the tracks a long queue had not drawn yet.
    const next = [...queued];
    const set = new URL(link.getAttribute("href"), location.origin).searchParams.get("in");
    if (set) next.push(new URL("/" + set, location.origin).href);
    // After the queue SoundCloud's autoplay goes on with the station of
    // the last track: related tracks.
    if (autoplay) {
      const last = new URL(queued.at(-1) ?? url.href);
      next.push(new URL("/stations/track" + last.pathname, location.origin).href);
    }
    const why = next.length ? "" : "nothing is queued, and SoundCloud's autoplay is off";
    return { url: url.href, time: Number.isFinite(time) ? time : 0, playing, next, why };
  }

  if (site("youtube.com") || host === "youtu.be") {
    const u = new URL(location.href);
    let id = u.searchParams.get("v");
    if (!id) id = u.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})/)?.[1];
    if (!id && host === "youtu.be") id = u.pathname.slice(1, 12);
    if (!id) return { error: "no YouTube video on this page" };
    // The player's own time is the video's, also while an ad plays in it;
    // the <video> element's would be the ad's. Firefox reaches the page's
    // functions through wrappedJSObject.
    const player = document.getElementById("movie_player");
    const api = player?.wrappedJSObject ?? player;
    let time, playing;
    if (typeof api?.getCurrentTime === "function") {
      time = api.getCurrentTime();
      playing = api.getPlayerState() === 1;
    } else {
      const v = document.querySelector("video.html5-main-video, video");
      if (!v) return { error: "no YouTube video on this page" };
      time = v.currentTime;
      playing = !v.paused;
    }
    // What comes after: the rest of the playlist it plays in (audweb
    // starts the list after this video); with no list and autoplay on, the
    // video autoplay has chosen, then YouTube's Mix for this video, which
    // goes on the way autoplay does.
    const watch = "https://www.youtube.com/watch?v=" + id;
    const next = [];
    let why = "";
    const list = u.searchParams.get("list");
    // The autoplay switch in the player. Not every player YouTube serves
    // has it there (some have it in the settings menu only); without it
    // autoplay is taken to be on, as it is unless switched off.
    const toggle = document.querySelector(".ytp-autonav-toggle-button");
    if (list) {
      next.push(watch + "&list=" + encodeURIComponent(list));
    } else if (!toggle || toggle.getAttribute("aria-checked") !== "false") {
      // Autoplay's choice: the player's own "watch next" data follows the
      // page from video to video; the page's first data (ytInitialData)
      // stays the first video's, so it is only used for that one.
      const win = window.wrappedJSObject ?? window;
      const sources = [];
      try {
        sources.push(api?.getWatchNextResponse?.());
      } catch (e) {}
      sources.push(win.ytInitialData);
      for (const data of sources) {
        if (data?.currentVideoEndpoint?.watchEndpoint?.videoId !== id) continue;
        const auto = data.contents?.twoColumnWatchNextResults?.autoplay?.autoplay
          ?.sets?.[0]?.autoplayVideo?.watchEndpoint?.videoId;
        if (auto && auto !== id) next.push("https://www.youtube.com/watch?v=" + auto);
        break;
      }
      next.push(watch + "&list=RD" + id);
    } else {
      why = "YouTube's autoplay is off";
    }
    // The video alone, with no &list= or &t=: the list comes after it.
    return { url: watch, time, playing, next, why };
  }

  // Elsewhere: the page itself, which yt-dlp may know, at the time of the
  // media element that plays.
  const media = [...document.querySelectorAll("video, audio")];
  const m = media.find((x) => !x.paused) ?? media.find((x) => x.currentTime > 0) ?? media[0];
  if (!m) return { error: "no video or audio on this page" };
  return { url: location.href, time: m.currentTime, playing: !m.paused, next: [], why: "only the track itself, on this site" };
}

// Runs in the page, once Audacious plays.
function pausePage() {
  const host = location.hostname;
  if (host === "soundcloud.com" || host.endsWith(".soundcloud.com")) {
    document.querySelector(".playControl.playing")?.click();
    return;
  }
  const player = document.getElementById("movie_player");
  const api = player?.wrappedJSObject ?? player;
  if (typeof api?.pauseVideo === "function") api.pauseVideo();
  for (const m of document.querySelectorAll("video, audio")) m.pause();
}

const TITLE = "Play in Audacious (Alt+Shift+A)";

function show(tabId, badge, title, color) {
  browser.action.setBadgeText({ tabId, text: badge });
  if (color) browser.action.setBadgeBackgroundColor({ tabId, color });
  browser.action.setTitle({ tabId, title });
}

function fail(tabId, error) {
  console.error("audweb:", error);
  show(tabId, "!", "audweb: " + error, "#c0392b");
}

const busy = new Set();

async function handOver(tab) {
  if (busy.has(tab.id)) return;
  busy.add(tab.id);
  try {
    show(tab.id, "…", "Starting Audacious…", "#5b6b7f");
    let page;
    try {
      [{ result: page }] = await browser.scripting.executeScript({
        target: { tabId: tab.id },
        func: readPage,
      });
    } catch (e) {
      fail(tab.id, "cannot read this page: " + e.message);
      return;
    }
    if (!page || page.error) {
      fail(tab.id, page?.error ?? "nothing found to play");
      return;
    }
    let reply;
    try {
      reply = await browser.runtime.sendNativeMessage("audweb", page);
    } catch (e) {
      // No manifest for "audweb", or audweb-host would not start.
      fail(tab.id, "audweb-host: " + e.message);
      return;
    }
    if (!reply?.ok) {
      fail(tab.id, reply?.error ?? "no answer from audweb-host");
      return;
    }
    if (page.playing) {
      await browser.scripting.executeScript({ target: { tabId: tab.id }, func: pausePage });
    }
    // Says what the last hand-over brought along, until the next one.
    show(tab.id, "", page.next.length
      ? `${TITLE}\nLast time: what follows is being added to Audacious`
      : `${TITLE}\nLast time: nothing follows (${page.why})`);
  } finally {
    busy.delete(tab.id);
  }
}

browser.action.onClicked.addListener(handOver);

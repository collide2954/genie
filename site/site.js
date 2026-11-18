// The scripts of the genie site. Each part works on the elements of its page and does nothing
// on other pages. The pages work without this script: it only adds to them.
(function () {
  "use strict";

  var dark = window.matchMedia("(prefers-color-scheme: dark)");
  var still = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Clips: the video of the look of the system, loaded when it comes near the screen and playing
  // only while it is on the screen. People who turn off motion get the controls and the last
  // frame as the poster.
  function clips() {
    var videos = document.querySelectorAll("video[data-clip]");
    if (!videos.length) return;
    function choose(video) {
      var look = dark.matches ? "dark" : "light";
      if (video.dataset.look === look) return;
      video.dataset.look = look;
      var name = video.dataset.clip;
      video.poster = "clips/" + name + "-" + look + ".webp";
      video.src = "clips/" + name + "-" + look + ".mp4";
    }
    function play(video) {
      var playing = video.play();
      if (playing && playing.catch) playing.catch(function () {});
    }
    var watcher = null;
    if (!still.matches && "IntersectionObserver" in window) {
      watcher = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          var video = entry.target;
          if (entry.isIntersecting) {
            choose(video);
            play(video);
          } else if (!video.paused) {
            video.pause();
          }
        });
      }, { threshold: 0.25, rootMargin: "120px 0px" });
    }
    videos.forEach(function (video) {
      if (watcher) {
        video.removeAttribute("controls");
        watcher.observe(video);
        video.addEventListener("click", function () {
          if (video.paused) play(video); else video.pause();
        });
      } else {
        choose(video);
        video.controls = true;
      }
    });
    dark.addEventListener("change", function () {
      videos.forEach(function (video) {
        if (!video.dataset.look) return;
        var playing = !video.paused;
        choose(video);
        if (playing) play(video);
      });
    });
  }

  // Download: the package for the system of the visitor, from the latest GitHub release.
  function downloads() {
    var box = document.getElementById("download");
    if (!box || !window.fetch) return;
    var systems = [
      { name: "Windows", test: /-windows-x64-setup\.exe$/ },
      { name: "macOS", test: /-macos-universal\.dmg$/ },
      { name: "Linux (AppImage)", test: /-linux-x86_64\.AppImage$/ },
      { name: "Linux (.deb)", test: /_amd64\.deb$/ }
    ];
    var agent = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || navigator.userAgent || "";
    var mine = /win/i.test(agent) ? 0 : /mac|iphone|ipad/i.test(agent) ? 1 : /linux|x11|ubuntu|fedora/i.test(agent) ? 2 : -1;
    function megabytes(bytes) { return Math.round(bytes / 1e6) + " MB"; }
    function link(system, asset, primary) {
      var a = document.createElement("a");
      a.className = "button" + (primary ? " primary" : "");
      a.href = asset.browser_download_url;
      a.textContent = (primary ? "Download for " : "") + system.name + " ";
      var size = document.createElement("span");
      size.className = "size";
      size.textContent = megabytes(asset.size);
      a.appendChild(size);
      return a;
    }
    fetch("https://api.github.com/repos/collide2954/genie/releases/latest")
      .then(function (answer) { return answer.ok ? answer.json() : Promise.reject(); })
      .then(function (release) {
        var assets = release.assets || [];
        var found = systems.map(function (system) {
          return assets.filter(function (asset) { return system.test.test(asset.name); })[0];
        });
        if (!found.some(Boolean)) return;
        var choices = box.querySelector(".choices");
        choices.textContent = "";
        if (mine >= 0 && found[mine]) choices.appendChild(link(systems[mine], found[mine], true));
        systems.forEach(function (system, index) {
          if (index !== mine && found[index]) choices.appendChild(link(system, found[index], false));
        });
        var version = String(release.tag_name || "").replace(/^v/, "");
        document.querySelectorAll("[data-version]").forEach(function (element) {
          element.textContent = "Version " + version + " for Windows, macOS and Linux.";
        });
      })
      .catch(function () {});
  }

  // Search in the manual: sections that have all the words, best titles first.
  function search() {
    var input = document.querySelector("input.search");
    var list = document.querySelector("ul.results");
    if (!input || !list) return;
    var index = null;
    function load() {
      if (index) return Promise.resolve(index);
      return fetch(input.dataset.index).then(function (answer) { return answer.json(); }).then(function (data) {
        index = data;
        return index;
      });
    }
    function show(query) {
      var words = query.toLowerCase().split(/\s+/).filter(Boolean);
      list.textContent = "";
      if (!words.length) return;
      load().then(function (sections) {
        var hits = sections.map(function (section) {
          var title = section.title.toLowerCase();
          var text = section.text.toLowerCase();
          var all = words.every(function (word) { return title.indexOf(word) >= 0 || text.indexOf(word) >= 0; });
          var score = words.filter(function (word) { return title.indexOf(word) >= 0; }).length;
          return { section: section, all: all, score: score };
        }).filter(function (hit) { return hit.all; });
        hits.sort(function (a, b) { return b.score - a.score; });
        if (!hits.length) {
          var none = document.createElement("li");
          none.className = "empty";
          none.textContent = "No section has these words.";
          list.appendChild(none);
          return;
        }
        hits.slice(0, 12).forEach(function (hit) {
          var item = document.createElement("li");
          var a = document.createElement("a");
          a.href = hit.section.url;
          a.textContent = hit.section.title;
          var where = document.createElement("span");
          where.className = "where";
          where.textContent = hit.section.page;
          a.appendChild(where);
          item.appendChild(a);
          list.appendChild(item);
        });
      });
    }
    input.addEventListener("input", function () { show(input.value); });
    input.addEventListener("keydown", function (event) {
      if (event.key === "Enter") {
        var first = list.querySelector("a");
        if (first) window.location.href = first.href;
      }
    });
  }

  clips();
  downloads();
  search();
})();

// The short cuts: under a minute, from the same scenes, in English (?cut=short)
// and Hindi (?cut=short-hi); render with `node render.mjs --cut short` or
// `--cut short-hi`. Each clip is a window of a scene, in that scene's own
// seconds, under a line of its own. The apps' interfaces stay in English.
// A clip with a line in only one of the languages plays only in that cut.
"use strict";

(() => {
  const clips = [
    { scene: "title", from: 0, to: 2.6, xfade: 0 },
    {
      scene: "cold-open", from: 1.2, to: 7.8, lead: 2.0,
      en: "A one-gigabyte spreadsheet, open in a third of a second.",
      hi: "एक गीगाबाइट की स्प्रेडशीट, सेकंड के तिहाई हिस्से में खुल गई।",
    },
    {
      scene: "brick-sort", from: 1.0, to: 5.4, lead: 0.3,
      en: "Brick sorts nineteen million rows in under a second.",
      hi: "Brick एक करोड़ नब्बे लाख पंक्तियों को एक सेकंड से भी कम में सॉर्ट कर देता है।",
    },
    {
      scene: "slate-open", from: 0.6, to: 6.0,
      en: "Slate is a word processor with no pages, and Word documents look like Word.",
      hi: "Slate एक वर्ड प्रोसेसर है जिसमें पन्ने ही नहीं हैं, और Word दस्तावेज़ बिल्कुल Word जैसे दिखते हैं।",
    },
    {
      scene: "slate-format", from: 1.6, to: 7.4,
      en: "A slim format bar, not a ribbon.",
      hi: "रिबन नहीं, बस एक पतला-सा फ़ॉर्मैट बार।",
    },
    {
      scene: "frame-shapes", from: 1.0, to: 8.6,
      en: "Frame edits PowerPoint decks right on the slide, with smart guides and one inspector.",
      hi: "Frame में PowerPoint डेक सीधे स्लाइड पर एडिट होते हैं, स्मार्ट गाइड और एक ही इंस्पेक्टर के साथ।",
    },
    {
      scene: "frame-outline", from: 1.0, to: 6.6,
      hi: "या बस एक आउटलाइन टाइप कीजिए।",
    },
    {
      scene: "etch-shapes", from: 0.6, to: 7.4,
      en: "Etch edits images, closer to Paint than to Photoshop.",
    },
    {
      scene: "install-terminal", from: 0.3, to: 5.0,
      en: "Each app installs with one command. No sudo, no account.",
      hi: "हर ऐप एक ही कमांड से इंस्टॉल होता है। न sudo, न अकाउंट।",
    },
    {
      scene: "close", from: 1.0, to: 12.16,
      en: "Cronkite: four apps, fast, native, and open source. Get them at cronkite.cc.",
      hi: "Cronkite. तेज़, नेटिव और ओपन सोर्स। cronkite.cc पर पाइए।",
    },
  ];
  const inLanguage = (lang) =>
    clips.filter((c) => (!c.en && !c.hi) || c[lang]).map(({ en, hi, ...clip }) => ({ ...clip, say: { en, hi }[lang] }));

  V.cut("short", inLanguage("en"));
  V.cut("short-hi", inLanguage("hi"), {
    lang: "hi",
    voice: "hi-IN-MadhurNeural",
    speed: 1, // Madhur's own pace; slower runs past a minute
    strings: {
      tagline: "Linux के लिए एक तेज़, नेटिव ऑफ़िस सुइट।",
      "open-stat": "<b>1 GB</b> CSV <b>314 ms</b> में खुली",
      "install-notes": ["सिर्फ़ आपके लिए: ~/.local", "sudo की ज़रूरत नहीं", "पैकेज मैनेजर नहीं", "sha256 से जाँच"],
      "close-line": "Brick, Slate, Frame और Etch · अभी उपलब्ध · ओपन सोर्स",
    },
  });
})();

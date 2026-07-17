# Aria 3.1 author language

Aria 3.1 is the author language required by a V3 1.0 release build. It is a
structured, UTF-8 visual-novel language that compiles completely to portable
.ariac bytecode. The Player never parses source text.

The design favors predictable writing over compatibility with the
line-oriented C# runtime: every pause, control transfer, asset reference, and
saved value is visible in source and checked before packaging.

## Minimal scene

~~~aria
aria 3.1;
module example.opening;
entry start;

state route: Int = 0;
state has_met_mio: Bool = false;

scene start {
  background asset("#07131f") with fade(240ms);
  show mio = image(asset("assets/ch/mio.webp")) at (760px, 86px) z 20;
  say ミオ: "潮風の音、聞こえる？";
  await advance;
  choice {
    "海辺へ行く" => shore;
    "ここで待つ" => stay;
  }
}

scene shore {
  route = 1;
  background asset("assets/bg/sea.webp") with wipe(420ms);
  play bgm asset("assets/audio/sea.ogg") loop fade 300ms;
  say ミオ: "それじゃあ、一緒に行こう。";
  await advance;
  end;
}

scene stay {
  route = 2;
  narrate "波音だけが、ゆっくり続いている。";
  await advance;
  end;
}
~~~

## Deliberate breaks from V1/V2 and alpha 3.0

- There are no percent/string registers, labels, strict mode, inline page
  controls, command-name aliases, or implicit numeric/string conversions.
- Dialogue does not silently wait. Write 'await advance;' where the reader
  should take control; typewriter, skip, auto, and replay timing stay visible.
- Scenes never fall through into the following scene. Every execution path
  ends with 'end', 'return', 'jump', or 'choice'.
- 'call' is only for a non-recursive subscene that returns on every path.
  'jump' and 'choice' targets must not return without a caller.
- Unsupported legacy commands never become a runtime no-op in a release
  build. 'aria migrate' either emits Aria 3.1 or reports the source location
  requiring an author decision.

## Source layout and modules

Every source starts with:

~~~aria
aria 3.1;
~~~

An optional 'module namespace.name;' documents a namespace. The project entry
source declares one 'entry scene_name;'. Imported modules use
'import "./common.aria";' and intentionally have no entry declaration.

Import paths are project-relative logical paths after resolution, use '/', and
may not escape the project. Scenes and saved-state names are global across the
import closure, so duplicate declarations are errors.

## Types and state

~~~aria
state chapter: Int = 1;       // persisted in a save
state seen: Bool = false;
state nickname: String = "ミオ";

scene example {
  let greeting: String = "こんにちは"; // immutable lexical binding
  var visits: Int = 0;               // mutable lexical binding
  visits += 1;
  if visits > 0 && !seen {
    seen = true;
  }
  end;
}
~~~

'state' is saved. 'let' and 'var' are lexical and exist only while their
scene/block executes. A name cannot be redeclared in the same block, and
assignment to a 'let' is an error. Conditions must be Bool; ordering
comparisons require Int.

The initial subset deliberately keeps expressions small: integer, boolean,
and string literals; variables in assignments/comparisons; '==', '!=', '<',
'<=', '>', '>=', '!', '&&', '||'; and '+=' with an integer literal. Arithmetic
expressions, collections, user functions, and plugins are future
language-version work rather than silently accepted syntax.

## Flow and interaction

~~~aria
scene route {
  choice {
    "はい" => yes;
    "いいえ" => no;
  }
}

scene yes {
  call record_yes;
  jump finish;
}

scene record_yes {
  return;
}

scene finish {
  end;
}
~~~

'choice', 'jump', 'return', and 'end' are terminal for the current scene
path. Code after a terminal transfer is rejected as unreachable. A 'while'
does not make an enclosing scene terminal because its condition may be false.

'wait 500ms;' advances deterministic logical time. 'await advance;' waits for
the normalized Advance/Confirm/pointer action. Native and Web receive the same
InputSnapshot action vocabulary, so a recorded stream can be replayed
identically.

The line-oriented front end used by migration and authoring tools follows the
same interaction rule: `名前「本文」` and `「地の文」` lower to display text
immediately, while `@`, `\`, or `await advance` lower to an explicit wait
(`\` also clears the page). Quoted and unquoted bare lines are accepted as
text only after directives, labels, assignments, block syntax, and reserved
commands have been classified. A line that has the shape of an unknown
command is a compiler error rather than accidental dialogue.

## Visuals, assets, audio, saves

~~~aria
background asset("assets/bg/sea.webp") with fade(300ms);
show mio = image(asset("assets/ch/mio.webp")) at (760px, 86px) z 20;
show panel = rect(40px, 520px, 1200px, 150px, "#07131fcc") z 5;
show title = text("海風") at (56px, 72px) size 42px z 10;
move mio to (720px, 86px);

play bgm asset("assets/audio/sea.ogg") loop fade 300ms;
volume bgm 0.70;
stop bgm fade 500ms;
save 1;
load 1;
~~~

Coordinates are logical pixels. The Player applies the same fit/safe-area
transform to rendering and input on Windows, Linux, and Web.

An asset reference is always 'asset("path")'. Its path must already be NFC,
project-relative, slash-separated, canonical, inside runtime.asset_roots, and
have exact packaged spelling. The compiler rejects case-only and Unicode
normalization collisions so a loose Windows project cannot differ from a
Linux package. Backgrounds may instead use a validated hex color such as
'asset("#07131f")'.

Fade and wipe are implemented transition kinds. Mask is parsed only to report
a clear error until every official renderer implements the same behavior.

The valid audio buses are 'bgm', 'se', and 'voice'. Playback, stop, loop,
fade, and volume lower to platform-neutral audio commands. Native and Web
storage transport saves outside the language with two valid generations.

## Release rule

~~~sh
aria check my-game --release
ARIA_PAK_SIGNING_KEY=publisher:<64-hex-bytes> \
  aria build my-game --target windows-x64 --profile signed --release
~~~

Both commands require Aria 3.1, a complete semantic compile with no Host
opcode, valid bundled assets/fonts, and a format-valid `.ariac`/`.ariapak`
bundle. `dev` is for local/test builds; release packaging must choose
`signed` or `protected` explicitly.
Development checks may inspect alpha 3.0 compatibility sources only during
migration; those sources are not deployable V3 1.0 artifacts.

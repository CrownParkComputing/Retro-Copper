import 'package:flutter/material.dart';

import '../emulator.dart';
import '../ffi/amiga_core.dart';
import '../theme/amiga_theme.dart';
import '../widgets/amiga_logo.dart';
import 'getting_started.dart';
import 'logs_panel.dart';

/// What this is, and what is running underneath it -- with the logs behind
/// a second tab, because "what went wrong" belongs next to "what is this".
/// The core actually loaded, asked at runtime rather than assumed. Both cores
/// answer the same interface on purpose, so this is the only honest way to
/// say which one is behind the app.
String _coreName() => AmigaCore.open()?.coreName ?? 'Amiberry';

bool _coreIsCopperline() => _coreName().toLowerCase().startsWith('copperline');

String _coreDescription() {
  if (_coreIsCopperline()) {
    return '${_coreName()}, a cycle-driven Amiga implementation written in '
        'Rust. It is built as a library for each platform and drives the '
        'chipset from the same clock the hardware did.';
  }
  return 'Amiberry, which is WinUAE\'s core with a Linux and handheld front '
      'end. It is vendored and built for each platform in turn, because they '
      'do not agree on much: iOS cannot JIT at all, so that build compiles '
      'the JIT out entirely rather than switching it off at runtime.';
}

class AboutPanel extends StatelessWidget {
  const AboutPanel({super.key});

  @override
  Widget build(BuildContext context) {
    // Help first. The questions this app gets are "where do I put my files"
    // and "why can't it see my SD card", and the answers were only ever
    // reachable from inside first-run setup -- which is behind the person
    // asking, not in front of them.
    return const DefaultTabController(
      length: 3,
      child: Column(
        children: <Widget>[
          TabBar(
            tabs: <Widget>[
              Tab(text: 'Help'),
              Tab(text: 'About'),
              Tab(text: 'Logs'),
            ],
            labelColor: AmigaColors.text,
            unselectedLabelColor: AmigaColors.textDim,
            indicatorColor: AmigaColors.workbenchOrange,
            dividerColor: AmigaColors.panelBorder,
          ),
          Expanded(
            child: TabBarView(
              children: <Widget>[
                _HelpBody(),
                _AboutBody(),
                LogsPanel(),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// The getting-started guide, on its own tab so it can be re-read.
///
/// The same steps the wizard shows, minus the ones that only make sense while
/// setup is running: what an Amiga needs, where this platform lets you put
/// files -- SD cards and USB drives included -- and how to start a game.
class _HelpBody extends StatelessWidget {
  const _HelpBody();

  @override
  Widget build(BuildContext context) {
    return GettingStartedGuide(
      steps: <GuideStep>[
        GettingStartedSteps.whatYouNeed(),
        GettingStartedSteps.whereFilesGo(),
        GettingStartedSteps.firstGame(),
      ],
      closeLabel: 'Start again',
    );
  }
}

class _AboutBody extends StatelessWidget {
  const _AboutBody();

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(18),
      children: <Widget>[
        Row(
          children: <Widget>[
            const AmigaLogo(height: 40),
            const SizedBox(width: 12),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                const Text(
                  'Retro-Amiga',
                  style: TextStyle(
                    fontSize: 22,
                    fontWeight: FontWeight.w700,
                    color: AmigaColors.text,
                  ),
                ),
                FutureBuilder<String>(
                  future: Emulator.platformName(),
                  builder: (BuildContext context, AsyncSnapshot<String> snap) {
                    return Text(
                      'running on ${snap.data ?? '...'}',
                      style: const TextStyle(color: AmigaColors.textDim),
                    );
                  },
                ),
              ],
            ),
          ],
        ),
        const SizedBox(height: 22),
        _Section(
          title: 'The emulator',
          body: _coreDescription(),
        ),
        if (_coreIsCopperline()) const _Section(
          title: 'How Copperline differs',
          body:
              'Amiberry descends from UAE, twenty-five years of C that grew a '
              'chipset as it went. Copperline is a fresh Rust implementation '
              'driven by the cycle, so the Copper, Blitter and Paula are timed '
              'against the same clock the real machine used rather than caught '
              'up with at the end of a scanline. What that buys is the things '
              'demos do on purpose and games do by accident: a colour changed '
              'mid-line lands where it was asked for, and a Blitter finishing '
              'inside the frame it was started in behaves like the hardware.\n\n'
              'It also carries what UAE bolted on later as first-class parts: '
              'real IDE and SCSI controllers rather than a virtual hard-drive '
              'device that bypasses the guest\'s own drivers, an RDB read from '
              'the image the way a real Amiga reads it, and directory mounts '
              'served live from a folder.\n\n'
              'Not everything has crossed over yet. Sound is silent, save '
              'states are not wired up, and JIT does not exist here at all - '
              'Copperline is fast enough without one on this hardware, but a '
              '68040 workload that leaned on Amiberry\'s JIT will feel it. '
              'Anything the machine configuration asks for that has no '
              'counterpart is reported in the log rather than quietly ignored.',
        ),
        const _Section(
          title: 'The front end',
          body:
              'Flutter, everywhere - Android, iOS, macOS, Linux and Windows '
              'from one codebase. The emulator draws its own screen natively; '
              'nothing is copied through Dart, so the picture costs what it '
              'does in a desktop emulator.',
        ),
        const _Section(
          title: 'What you need to supply',
          body:
              'A Kickstart ROM, for full compatibility. The app boots '
              'without one using the bundled AROS ROM, but AROS is a '
              'reimplementation rather than a clone, so most WHDLoad games '
              'and some floppies still want the real thing. Kickstarts are '
              'under copyright, so none ships here - Cloanto sell them as '
              'Amiga Forever, and if you have an Amiga you may already own '
              'one. Everything else, the app will find by scanning.',
        ),
        const _Section(
          title: 'AROS',
          body:
              'The fallback Kickstart is the AROS m68k ROM, an independent '
              'open reimplementation of AmigaOS, used here under the AROS '
              'Public License. Source and the licence text are at '
              'aros.sourceforge.io. WHDLoad, JST and AmiQuit are Bert Jahn\'s '
              'and ship under their own terms.',
        ),
        _Section(
          title: 'Thanks',
          body: _coreIsCopperline()
              ? 'Copperline, whose core this runs on; Amiberry and WinUAE, '
                  'which carried the Amiga this far; and the people who wrote '
                  'the demos that made anyone want a machine like this in the '
                  'first place.'
              : 'Amiberry, WinUAE, and the people who wrote the demos that '
                  'made anyone want a machine like this in the first place.',
        ),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.body});

  final String title;
  final String body;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            title,
            style: const TextStyle(
              fontSize: 15,
              fontWeight: FontWeight.w700,
              color: AmigaColors.accent,
            ),
          ),
          const SizedBox(height: 5),
          Text(
            body,
            style: const TextStyle(
              fontSize: 13,
              height: 1.4,
              color: AmigaColors.textDim,
            ),
          ),
        ],
      ),
    );
  }
}

# Linux packages: apt, dnf, zypper, apk, AUR, Nix, Snap, Gentoo

Deferred 2026-10-07. When `screen-reader-mode` lands, this file gets its row in
that branch's `docs/backlog/README.md`.

**What.** tmux-companion installable with each Linux distribution's own
package manager, the way it is with Homebrew and crates.io today:
`apt install`, `dnf`/`yum install`, `zypper install`, `apk add`, the AUR,
Nix, Snap and Gentoo.

**Why it waits.** Nobody has asked yet, and the install script, Homebrew and
`cargo install` already cover Linux. It is about three days of work for the
first set below, and a signing key to look after from then on.

**What's known.** Two kinds of channel, and only one keeps up with several
releases a day.

- *Official archives* (`apt-get install tmux-companion` with nothing added
  first): a maintainer inside each distribution packages it through review
  and ships on that distribution's schedule, days for Arch, Alpine and
  nixpkgs, months for Fedora and openSUSE, years for Debian or Ubuntu
  stable. Debian also wants every Rust dependency packaged on its own. None
  of that is ours to run and none of it follows several releases a day.
  These usually come later, from distribution packagers, once a tool is
  used.
- *Channels we publish to* from `release.yml`: the user adds the repository
  once, then installs and upgrades with the usual command. These keep pace
  with any number of releases, the way Homebrew and crates.io do.

| People get | Channel | Work once | Ongoing |
| --- | --- | --- | --- |
| `apt install` (Ubuntu, Debian) | `.deb` built in `release.yml` with nfpm, in a signed apt repository on R2 (media.lonkar.org's bucket) or Cloudsmith | 1–2 days with the key | none per release |
| `dnf`/`yum install`, `zypper install` (Fedora, RHEL, CentOS, openSUSE) | `.rpm` from the same nfpm step, in a signed rpm repository beside it | in the line above | none |
| `apk add` (Alpine) | `.apk` from the same step, in a signed repository of our own | half a day | none |
| `yay -S tmux-companion-bin` (Arch, Manjaro) | an AUR `-bin` package that CI pushes | 2–3 hours | none; `pacman -S` itself is the official repositories only |
| `nix profile install github:lonkar-org/tmux-companion` | a flake in this repository | half a day | none; it builds the commit asked for |
| `snap install` | the Snap Store, with stable and edge channels | about a day, and a one-time review for classic confinement, which it needs to reach tmux's socket and your files | none |
| `emerge` (Gentoo) | an overlay of our own, one ebuild per version bumped by a script | half a day | none |

What makes it cheap: every release already builds static musl binaries for
x86_64 and aarch64 Linux, so packaging is wrapping a binary that runs on any
distribution, with no compiling per distribution, and nfpm makes `.deb`,
`.rpm` and `.apk` from one config. The cost that stays is the signing keys:
made once, kept as CI secrets, rotated, and published for people to trust.

Several releases a day reach `apt upgrade` as several upgrades a day. A
stable and an edge channel (Snap has them built in, an apt or rpm repository
can carry two suites) let every release reach edge and only chosen ones reach
stable.

**Verdict, 2026-10-07: not yet, except two cheap pieces.** Nobody has asked:
crates.io shows 7 downloads and no issue mentions packaging. Linux is already
covered by the install script, which checks checksums, by Homebrew on Linux
and by `cargo install`, and the people this is for, developers running coding
agents, use any of those without trouble. A repository we host doesn't bring
anybody new, since they still have to find the project and add the
repository first; only the official archives do that, and those aren't ours
to join. Against that sit the signing keys and the distribution-specific
install problems, for a tool that changes several times a day.

The two worth doing when there's a spare hour or two, since neither needs a
key or a host:

- a Nix flake, half a day: `nix profile install github:lonkar-org/tmux-companion`
  is what Nix users expect of a repository, and they are common among the
  people this is for;
- `.deb` and `.rpm` attached to each GitHub release with nfpm, about two
  hours: `apt install ./tmux-companion_<version>_amd64.deb` with no repository
  to run, and the first step toward one if it's ever wanted.

**What brings it back.** Somebody asking for one of these, or Linux users
showing up in issues with an install that the script or Homebrew made awkward.
Start with nfpm in `release.yml`, the apt and rpm repositories on R2, the AUR
package and the flake; Snap and Gentoo when somebody wants them.

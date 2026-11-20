#!/usr/bin/env python3
"""Render the two install prompts for the README and website from one set of templates.

  docs/install-prompts/1-card.tmpl     the one prompt the user pastes: build ANGI, write the SD card, hand off
  docs/install-prompts/2-connect.tmpl  part 2, given to the user by the agent when part 1 is done: get the system ready
  scripts/boards.json                  per-target wording

Writes:
  docs/install-prompts/part2-BOARD.txt  one rendered part 2 per target; part 1 tells the agent to print it
  README.md                 between <!-- prompt1:start/end -->                (generic prompt)
  website/get-started.html  <pre ... data-prompt="1">                         (generic prompt)
  website/index.html        <script type="application/json" id="targets-data">  (part 1 per target)
Run after editing a template or boards.json:  python3 scripts/sync-install-prompt.py
"""
import html, json, re, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
T1 = (ROOT / "docs/install-prompts/1-card.tmpl").read_text().rstrip("\n")
T2 = (ROOT / "docs/install-prompts/2-connect.tmpl").read_text().rstrip("\n")
T2_PI4 = (ROOT / "docs/install-prompts/2-connect-rpi4.tmpl").read_text().rstrip("\n")
boards = json.loads((ROOT / "scripts/boards.json").read_text())["boards"]

ASK = """STEP 0 - ASK WHAT ANGI IS GOING ON
If I have not already said, ask me this and wait for the answer:
  1) Raspberry Pi Zero 2 W        (rpizero2w)
  2) Raspberry Pi 4 or 400        (rpi4)
  3) Raspberry Pi 5               (rpi5)
  4) ARM desktop or laptop        (arm-desktop)
  5) Intel/AMD desktop or laptop  (x86-desktop)
  6) Not sure - help me tell
If I answer 6, run scripts/detect-board.py --list and --host and show me the "how to
tell" lines (for a Pi, ask me to read the name printed on the board). Then run
scripts/detect-board.py --require BOARD and follow what it reports. If it says a
target has no image yet or has not been tried on hardware, tell me plainly and
continue only if I agree. Never write another target's image to my drive, and never
wipe this computer's disks or any disk I did not choose.
"""
PICK = """STEP 0 - TARGET
I am installing ANGI on: {name} (BOARD={id}). Do not ask me which one.
Run scripts/detect-board.py --require {id}. If it reports that there is no image for it,
stop and tell me so in plain words.
"""
PI_STEPS = ("Power off the Pi and unplug it.",
            "Take the finished card out of the card reader on this computer.",
            "Push the card into the Pi's microSD slot (gold contacts toward the board) until it seats.")
PI_WAIT = "Wait up to 2 minutes for it to start: the red light is power, a blinking green light means it is reading the card."


def steps(*items):
    return "\n".join(f"  {i}) {t}" for i, t in enumerate(items, 1))


GENERIC_HANDOFF = steps(
    "Power off the target (a desktop: shut it down).",
    "Take the finished card out of the card reader on this computer.",
    "Put it in the target: push a microSD card into the Pi's slot, gold contacts toward the board, until it seats. "
    "A desktop: plug the drive in.",
    "Connect it to this computer by Ethernet or USB-C data cable (desktops: no cable), then power it on.",
    "Wait up to 2 minutes for the first start. A Pi shows a red light, then a blinking green one.")
LINK_GENERIC = """Ask which link I use, if I did not say:
  USB-C (any Pi): USB-C data cable. Pi 10.77.0.2, me 10.77.0.1.
  ETHERNET (Pi 4 / Pi 5): RJ45 cable and the Pi's own power supply. Pi 10.78.0.2, me 10.78.0.1.
  DESKTOP (ARM or Intel/AMD): no cable. Help me find it on my network or use its screen.
USB-C: create or reuse the NetworkManager profile agx-direct (MAC
02:77:00:00:00:01). ETHERNET: create or reuse agx-ethernet on my wired port that is NOT
carrying my internet; if that is my only wired port, stop and ask. Both: no gateway,
ipv4.never-default yes, IPv6 off, autoconnect yes. Do not touch my internet connection."""
LINK_USBC_ETH = """Ask me if I am using USB-C or Ethernet, if I did not say.
USB-C: data cable to the Pi's USB-C port. Pi 10.77.0.2, me 10.77.0.1. Create or reuse the
NetworkManager profile agx-direct (MAC 02:77:00:00:00:01).
ETHERNET: RJ45 cable and the Pi's own power supply. Pi 10.78.0.2, me 10.78.0.1. Create or
reuse agx-ethernet on my wired port that is NOT carrying my internet; if that is my only
wired port, stop and ask.
Both: no gateway, ipv4.never-default yes, IPv6 off, autoconnect yes. Do not touch my
internet connection."""
LINK_ZERO = """The Pi Zero 2 W has no Ethernet jack. I use a USB-C data cable.
Pi 10.77.0.2, me 10.77.0.1. Create or reuse the NetworkManager profile agx-direct (MAC
02:77:00:00:00:01): no gateway, ipv4.never-default yes, IPv6 off, autoconnect yes. Do not
touch my internet connection."""
LINK_DESKTOP = """There is no cable profile for this target. Ask me how it is reachable (it is on my network,
or I have its screen and keyboard) and help me connect that way."""
FIND_PI = """Wait up to 2 minutes for it to boot. For USB, look for a new network adapter (usually
enx...) and bring up agx-direct; for Ethernet bring up agx-ethernet. Confirm the route and
ping it. If it does not appear, go through: data cable (not charge-only)? powered, with the
red LED on and the green LED blinking? another USB port or the Pi's own supply? card fully
seated? Fix what you can and tell me what to check."""
FIND_DESKTOP = """Wait for it to start from the drive. Help me find its address (ask me, or check my router
or the screen) and confirm I can reach it. If it does not start, help me find the boot menu
key and choose the drive."""
EXTRA_PI5 = "4. If a monitor is attached, offer to open the desktop (agx desktop).\n"
WIFI_ZERO = "\nThe Pi Zero 2 W only supports 2.4 GHz Wi-Fi, so help me use a 2.4 GHz network."


def render(t, vals):
    for k, v in vals.items():
        t = t.replace("{{" + k + "}}", v)
    left = re.findall(r"{{\w+}}", t)
    if left:
        sys.exit(f"unfilled placeholders: {left}")
    return t


def generic():
    p1 = render(T1, dict(WHAT="I will tell you which computer ANGI is for.", MEDIA="SD card or USB drive", MEDIA_UP="CARD",
                         ATTACHED="my microSD card (or SD card or USB drive) is attached to THIS computer, in a card reader",
                         STEP0=ASK, HANDOFF=GENERIC_HANDOFF))
    p2 = render(T2, dict(WHAT="My ANGI card from part 1 is in the target and it is connected to this computer.",
                         STEP0="STEP 0 - WHICH ONE\nAsk me which target this is (Pi Zero 2 W, Pi 4 or 400, Pi 5, ARM computer, Intel/AMD computer) if\nI did not say, and run scripts/detect-board.py --require BOARD. Follow what it reports.\n",
                         LINK=LINK_GENERIC, FIND=FIND_PI, EXTRA="", WIFI_NOTE=""))
    return p1, p2


def for_target(b):
    name, bid = b["marketing"]["short"], b["id"]
    desktop = bid.endswith("desktop")
    media = "drive" if desktop else "card"
    attached = ("my SD card or USB drive is attached to THIS computer" if desktop
                else "a card reader with my microSD card is attached to THIS computer")
    if desktop:
        handoff = steps("Unplug the finished drive from this computer.",
                        "Plug it into the computer ANGI is going on, and start that computer from the drive (boot menu).",
                        "Wait for it to start.")
    elif bid == "rpizero2w":
        handoff = steps(*PI_STEPS, "Connect a USB-C DATA cable (not charge-only) from the Pi Zero 2 W's data port to this "
                        "computer. This also powers it.", PI_WAIT)
    elif bid == "rpi4":
        handoff = steps(*(t.replace("the Pi", f"the {name}") for t in PI_STEPS),
                        "Connect a USB-C data cable to this computer and power on the Pi. For direct Ethernet, connect the Ethernet cable and use the Pi's own power supply.", PI_WAIT)
    else:
        handoff = steps(*(t.replace("the Pi", f"the {name}") for t in PI_STEPS),
                        "Connect it to this computer with an Ethernet cable (then use the Pi's own power supply) or a "
                        "USB-C DATA cable (not charge-only), and power it on.", PI_WAIT)
    step0 = PICK.format(name=name, id=bid)
    p1 = render(T1, dict(WHAT=f"The target is my {name}.", MEDIA=("SD card or USB drive" if desktop else "microSD card"),
                         MEDIA_UP=media.upper(), ATTACHED=attached, STEP0=step0, HANDOFF=handoff))
    if bid == "rpi4":
        return p1, render(T2_PI4, dict(STEP0=step0))
    link = LINK_DESKTOP if desktop else LINK_ZERO if bid == "rpizero2w" else LINK_USBC_ETH
    p2 = render(T2, dict(WHAT=f"The target is my {name}. The card from part 1 is in it and it is connected to this computer.",
                         STEP0=step0, LINK=link, FIND=FIND_DESKTOP if desktop else FIND_PI,
                         EXTRA=EXTRA_PI5 if bid == "rpi5" else "",
                         WIFI_NOTE=WIFI_ZERO if bid == "rpizero2w" else ""))
    return p1, p2


def sub_block(path, pattern, repl):
    text = path.read_text()
    text, n = re.subn(pattern, repl, text, flags=re.S)
    if n != 1:
        sys.exit(f"{path.name}: expected one match for {pattern[:40]!r}, found {n}")
    path.write_text(text)


g1, g2 = generic()
readme = ROOT / "README.md"
for i, g in ((1, g1),):
    sub_block(readme, rf"(<!-- prompt{i}:start -->\n```text\n).*?(\n```\n<!-- prompt{i}:end -->)",
              lambda m, g=g: m.group(1) + g + m.group(2))
gs = ROOT / "website/get-started.html"
for i, g in ((1, g1),):
    sub_block(gs, rf'(<pre\b[^>]*\bdata-prompt="{i}"[^>]*>).*?(</pre>)',
              lambda m, g=g: m.group(1) + html.escape(g, quote=True) + m.group(2))

OUT = ROOT / "docs/install-prompts"
targets = []
for b in boards:
    a, c = for_target(b)
    (OUT / f"part2-{b['id']}.txt").write_text(c + "\n")
    targets.append({"id": b["id"], **{k: b["marketing"][k] for k in ("short", "tagline", "media", "card", "reader", "connect")},
                    "prompt1": a})
payload = json.dumps(targets, indent=1, ensure_ascii=False).replace("</", "<\\/")
sub_block(ROOT / "website/index.html", r'(<script type="application/json" id="targets-data">).*?(</script>)',
          lambda m: m.group(1) + payload + m.group(2))
print("Prompts synced: README.md, website/get-started.html, website/index.html (part 1 per target; part 2 files in docs/install-prompts).")

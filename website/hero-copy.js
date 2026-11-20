// Keep the first option in HTML so the hero also works without JavaScript.
(() => {
  const title = document.getElementById('hero-title');
  const eyebrow = document.getElementById('hero-eyebrow');
  const description = document.getElementById('hero-description');
  const hint = document.getElementById('hero-copy-hint');
  const number = document.getElementById('hero-copy-number');
  const total = document.getElementById('hero-copy-total');
  if (!title || !eyebrow || !description || !hint || !number || !total) return;

  const lines = [...title.querySelectorAll('span')];
  const options = [
    {
      eyebrow: eyebrow.textContent,
      title: lines.map(line => line.textContent),
      description: description.textContent,
    },
    {
      eyebrow: 'Linux underneath. ANGI from there.',
      title: ['Start with less.', 'Make room for more.'],
      description: 'ANGI is assembled from the Linux kernel up, without a distribution underneath. Its own Rust core and Hyprland-style desktop keep the system small, leaving room for your projects.',
    },
    {
      eyebrow: 'The tiling desktop, trimmed down',
      title: ['Less OS.', 'More elbow room.'],
      description: 'Windows tile into place. Keyboard shortcuts keep you moving. ANGI brings the feel of Hyprland to its own small desktop, built for a Raspberry Pi 5 and the agents you work with.',
    },
    {
      eyebrow: 'A big ambition for a small OS',
      title: ['How small', 'can a desktop go?'],
      description: 'We’re building toward the smallest practical OS for a tiling desktop and AI coding agents. ANGI starts with Linux, adds only what it needs, and runs on a single Raspberry Pi 5. Come look inside.',
    },
    {
      eyebrow: 'Small by design, from the start',
      title: ['Keep the desktop.', 'Lose the weight.'],
      description: 'ANGI starts with the parts it needs: Linux, a Rust system core, and a desktop with Hyprland-style tiling. Built from source for the Raspberry Pi 5, it keeps the everyday tools close at hand.',
    },
    {
      eyebrow: 'Your Pi has room for this',
      title: ['A little board.', 'A place to build.'],
      description: 'Turn a Raspberry Pi 5 into a workspace with tiled windows, a terminal, a browser, and AI coding agents. ANGI brings them together in a small OS you can read and rebuild.',
    },
    {
      eyebrow: 'Built up from Linux',
      title: ['Every part', 'earns its place.'],
      description: 'ANGI’s system image is assembled from source with Buildroot. We choose what goes in, from startup services to the tiling desktop, so you can see what runs on your Pi and why it’s there.',
    },
    {
      eyebrow: 'Familiar moves. A smaller system.',
      title: ['Tile your windows.', 'Find your flow.'],
      description: 'Use the keyboard to move between windows and workspaces, with the tiling habits you know from Hyprland. Underneath is ANGI’s own small Linux system, built for a Raspberry Pi 5.',
    },
    {
      eyebrow: 'The whole system is within reach',
      title: ['Small enough', 'to get to know.'],
      description: 'Follow ANGI from startup to desktop in one repository. Its Rust core, system services, and window manager are there to explore, with AI coding tools to help you make changes.',
    },
    {
      eyebrow: 'A fresh start for the desktop',
      title: ['Linux at heart.', 'Small on purpose.'],
      description: 'ANGI builds directly on Linux, with its own services and Hyprland-style desktop. There’s no underlying distribution to carry along. Just the parts we choose for this little computer.',
    },
    {
      eyebrow: 'See what a small OS can do',
      title: ['Think Big.', 'Build small.'],
      description: 'One Raspberry Pi 5. A tiling desktop. Tools for you and your coding agents. ANGI keeps the foundation lean and open, so you can start with a small system and make it your own.',
    },
  ];

  let current = 0;
  title.addEventListener('click', () => {
    current = (current + 1) % options.length;
    const option = options[current];
    eyebrow.textContent = option.eyebrow;
    lines.forEach((line, index) => { line.textContent = option.title[index]; });
    description.textContent = option.description;
    number.textContent = String(current + 1);
  });

  total.textContent = String(options.length);
  title.disabled = false;
  hint.hidden = false;
})();

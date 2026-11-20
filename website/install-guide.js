(function(){
    const data=JSON.parse(document.getElementById('targets-data').textContent);
    const by=Object.fromEntries(data.map(t=>[t.id,t]));
    const guide=document.getElementById('guide'),dlg=document.getElementById('pdlg'),pre=document.getElementById('pdlg-pre'),copy=document.getElementById('pdlg-copy');
    const esc=t=>t.replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
    let current=null;
    function setupSteps(target) {
      if (target.id === 'rpi4') {
        return `<li><span class="n" aria-hidden="true">2</span><div><h4>Boot and connect over SSH</h4><p>Move the verified card into the powered-off Pi. Connect a USB-C data cable and power on. Direct Ethernet is the fallback. In a new chat, paste the second prompt your agent gives you and confirm the Pi’s SSH fingerprint.</p></div></li>
          <li><span class="n" aria-hidden="true">3</span><div><h4>Create your account and join Wi-Fi</h4><p>Your agent opens the ANGI TUI over SSH. Use <b>Profile</b> to create your administrator, then <b>Network</b> to join Wi-Fi. Enter passwords in the terminal.</p></div></li>
          <li><span class="n" aria-hidden="true">4</span><div><h4>Open the desktop over VNC</h4><p>After checking Wi-Fi, your agent opens an SSH tunnel. Connect a VNC viewer for your operating system to the local tunnel port and sign in with your new administrator account.</p><p><a href="get-started.html#pi4-workflow">See the full Pi 4 workflow</a></p></div></li>`;
      }
      return `<li><span class="n">2</span><div><h4>Move the card and connect</h4><p>${esc(target.connect)}</p></div></li>
        <li><span class="n">3</span><div><h4>Join Wi-Fi and get the system ready</h4><p>When the card is finished your agent gives you a second prompt. Paste it into a new chat. It connects to your ${esc(target.short)}, then <b>joins it to your Wi-Fi</b> and checks that it can reach the web. Have your network name, password and country code ready.</p><p class="soft">It also sets up your account and checks every service.</p></div></li>`;
    }
    function pick(id,scroll){
      const t=by[id]; if(!t) return; current=t;
      document.querySelectorAll('.tcard').forEach(c=>{const on=c.dataset.pick===id;c.classList.toggle('sel',on);c.setAttribute('aria-pressed',on);});
      guide.hidden=false;
      guide.innerHTML=`<h3 class="grad">Install ANGI on your ${esc(t.short)}</h3>
        <p class="gintro">Open any agent that can run commands (<b class="or">Codex</b>, <b class="or">Claude Code</b>, <b class="or">Cursor</b>, <b class="or">OpenCode</b>, <b class="or">Hermes</b>, any harness or IDE) on your computer and paste the install prompt into its chat.</p>
        <ol class="gsteps">
          <li><span class="n" aria-hidden="true">1</span><div><h4>Prepare the ${esc(t.media)} on your computer</h4><p>${esc(t.card)}</p><p>${esc(t.reader)}</p><p class="soft">${t.id === 'rpi4' ? 'Your agent builds sdcard.img with your SSH public key, checks its SHA-256, then verifies the card byte for byte after you approve the erase.' : 'The prompt builds ANGI and writes it to the card. Your agent shows you the card and asks before it erases anything.'}</p><button type="button" class="btn primary" data-open="prompt1">Open the prompt <svg class="i"><use href="#i-arrow"/></svg></button></div></li>
          ${setupSteps(t)}
        </ol>`;
      guide.querySelectorAll('[data-open]').forEach(b=>b.addEventListener('click',()=>openPrompt(b.dataset.open)));
      if(history.replaceState) history.replaceState(null,'','#install-'+id);
      if (scroll) {
        const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
        guide.scrollIntoView({behavior: reduceMotion ? 'instant' : 'smooth', block:'start'});
      }
    }
    function openPrompt(which){
      if(!current) return;
      document.getElementById('pdlg-title').textContent='Install prompt for '+current.short;
      pre.textContent=current[which]; copy.textContent='Copy prompt';
      dlg.showModal ? dlg.showModal() : dlg.setAttribute('open','');
    }
    document.querySelectorAll('[data-pick]').forEach(el=>el.addEventListener('click',e=>{e.preventDefault();pick(el.dataset.pick,true);}));
    copy.addEventListener('click',async()=>{try{await navigator.clipboard.writeText(pre.textContent);copy.textContent='Copied';}catch(e){const r=document.createRange();r.selectNodeContents(pre);const sel=getSelection();sel.removeAllRanges();sel.addRange(r);copy.textContent='Press Ctrl+C';}setTimeout(()=>copy.textContent='Copy prompt',2200);});
    const close=()=>dlg.close?dlg.close():dlg.removeAttribute('open');
    document.getElementById('pdlg-x').addEventListener('click',close);
    dlg.addEventListener('click',e=>{if(e.target===dlg)close();});
    const m=location.hash.match(/^#install-([\w-]+)$/); if(m&&by[m[1]]) pick(m[1],true);
  })();

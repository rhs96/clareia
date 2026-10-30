const $ = id => document.getElementById(id);
let previous, current, apps = [], history = [], paused = false, all = false, busy = false;
let sessionStart, totalDown = 0, totalUp = 0, generation = 0;
const sustained = new Map();
const fmt = n => {
  if (!Number.isFinite(n)) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB']; let i = 0;
  while (n >= 1024 && i < 4) { n /= 1024; i++; }
  return `${n.toLocaleString('pt-BR', { maximumFractionDigits: 1 })} ${units[i]}`;
};
const pct = n => Number.isFinite(n) ? n.toLocaleString('pt-BR', { maximumFractionDigits: 1 }) + '%' : '—';
const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const processKey = p => `${p.pid}:${p.startedTicks || p.started || 'unknown'}`;
function bytes(traffic, protocol = '') {
  if (!traffic) return { sent: 0, received: 0 };
  if (protocol === 'TCP') return { sent: traffic.tcpSent, received: traffic.tcpReceived };
  if (protocol === 'UDP') return { sent: traffic.udpSent, received: traffic.udpReceived };
  return { sent: traffic.sent, received: traffic.received };
}
function derive(now, old) {
  const dt = old ? (now.time - old.time) / 1000 : 0;
  const prior = new Map((old?.processes || []).map(p => [processKey(p), p]));
  const traffic = now.network?.processes || {};
  const pastTraffic = old?.network?.processes || {};
  const networkDt = old ? ((now.network?.sampleTime || now.time) - (old.network?.sampleTime || old.time)) / 1000 : 0;
  const canRate = now.network?.running && old?.network?.running && !old.network.paused && networkDt > 0;
  const processes = new Map(now.processes.map(p => [processKey(p), p]));
  for (const [key, p] of Object.entries(traffic)) {
    if (!processes.has(key)) processes.set(key, { ...p, started: p.startedTicks, cpu: null, memory: null, gpu: null, archived: true });
  }
  return [...processes].map(([key, p]) => {
    const prev = prior.get(key); const net = traffic[key];
    const rates = {};
    for (const protocol of ['', 'TCP', 'UDP']) {
      const a = bytes(net, protocol), b = bytes(pastTraffic[key], protocol);
      rates[protocol] = {
        sent: canRate ? Math.max(0, a.sent - b.sent) / networkDt : null,
        received: canRate ? Math.max(0, a.received - b.received) / networkDt : null
      };
    }
    return {
      ...p, key, traffic: net, rates,
      cpuPercent: !p.archived && prev && dt > 0 ? Math.min(100, Math.max(0, (p.cpu - prev.cpu) / dt / now.cores * 100)) : null,
      connections: p.archived ? [] : now.connections.filter(c => c.pid === p.pid),
      sockets: p.archived ? [] : (now.udp || []).filter(c => c.pid === p.pid)
    };
  });
}
function draw() {
  const max = Math.max(1024, ...history.flatMap(p => [p.down, p.up]));
  const line = key => history.map((p, i) => `${i / 59 * 1000},${135 - p[key] / max * 125}`).join(' ');
  $('chart').innerHTML = `<polyline points="${line('down')}" fill="none" stroke="#378963" stroke-width="2.5"/><polyline points="${line('up')}" fill="none" stroke="#7097c7" stroke-width="2.5"/>`;
}
function render() {
  const query = $('search').value.toLocaleLowerCase(), protocol = $('protocol').value || '';
  const list = apps.filter(p => {
    const network = bytes(p.traffic, protocol);
    const r=p.rates[protocol]; const mode=$('trafficMode').value || 'now'; const active=mode==='session' ? network.sent+network.received>0 : (r.sent||0)+(r.received||0)>0;
    return (all || active) && `${p.name} ${p.pid} ${Object.values(p.traffic?.flows || {}).map(f => f.remote).join(' ')}`.toLocaleLowerCase().includes(query);
  });
  const sort = $('sort').value || 'network';
  list.sort((a, b) => sort === 'cpu' ? (b.cpuPercent || 0) - (a.cpuPercent || 0) : sort === 'gpu' ? (b.gpu || 0) - (a.gpu || 0) :
    ((b.rates[protocol].sent || 0) + (b.rates[protocol].received || 0)) - ((a.rates[protocol].sent || 0) + (a.rates[protocol].received || 0)) ||
    bytes(b.traffic, protocol).sent + bytes(b.traffic, protocol).received - bytes(a.traffic, protocol).sent - bytes(a.traffic, protocol).received);
  $('rows').innerHTML = list.map(p => {
    const n = bytes(p.traffic, protocol), r = p.rates[protocol]; const measured = current?.network?.running;
    return `<tr><td><button data-key="${esc(p.key)}">${esc(p.name)}</button><small>PID ${p.pid}${p.archived ? ' · encerrado ou identidade indisponível' : ''}</small></td>
    <td>${measured ? fmt(r.received) + '/s' : '—'}<small>${measured || p.traffic ? fmt(n.received) + ' na sessão' : 'Ative a medição'}</small></td>
    <td>${measured ? fmt(r.sent) + '/s' : '—'}<small>${measured || p.traffic ? fmt(n.sent) + ' na sessão' : 'Ative a medição'}</small></td>
    <td>${pct(p.cpuPercent)}</td><td>${pct(p.gpu)}</td><td>${fmt(p.memory)}</td><td>${p.connections.length} TCP<small>${p.sockets.length} sockets UDP</small></td></tr>`;
  }).join('');
  $('empty').hidden = !!list.length;
}
function details(key) {
  const p = apps.find(x => x.key === key); if (!p) return;
  $('detailName').textContent = `${p.name} · PID ${p.pid}`;
  const flows = Object.values(p.traffic?.flows || {}).sort((a, b) => b.sent + b.received - a.sent - a.received);
  $('detailBody').innerHTML = `<p>Executável</p><code>${esc(p.path || 'Caminho indisponível para este processo.')}</code>
    <p>CPU: ${pct(p.cpuPercent)} · GPU: ${pct(p.gpu)} · memória: ${fmt(p.memory)}</p>
    ${p.gpuEngine ? `<p class="note">Motor mais ocupado: ${esc(p.gpuEngine)}</p>` : ''}
    <h3>Tráfego observado nesta sessão</h3><p>TCP: ↓ ${fmt(p.traffic?.tcpReceived)} · ↑ ${fmt(p.traffic?.tcpSent)}<br>UDP: ↓ ${fmt(p.traffic?.udpReceived)} · ↑ ${fmt(p.traffic?.udpSent)}</p>
    <p>QUIC é transportado por UDP e entra nessa contagem. A coleta de metadados não confirma qual protocolo de aplicação está em uso.</p>
    <div class="tablewrap"><table><thead><tr><th>PROTOCOLO</th><th>LOCAL → REMOTO</th><th>RECEBIDO</th><th>ENVIADO</th></tr></thead><tbody>
    ${flows.map(f => `<tr><td>${esc(f.protocol)}</td><td>${esc(f.local)}:${f.localPort}<br>→ ${esc(f.remote)}:${f.port}</td><td>${fmt(f.received)}</td><td>${fmt(f.sent)}</td></tr>`).join('')}</tbody></table></div>
    ${!flows.length ? '<p>Nenhuma transferência registrada para esta identidade de processo.</p>' : ''}
    <p class="note">Até 64 fluxos recentes por processo. Os totais por programa continuam acumulados mesmo quando um fluxo antigo sai desta lista.</p>
    <h3>Conexões TCP estabelecidas agora</h3><ul>${p.connections.map(c => `<li>${esc(c.local)}:${c.localPort} → ${esc(c.remote)}:${c.port}</li>`).join('') || '<li>Nenhuma nesta amostra.</li>'}</ul>
    <h3>Sockets UDP abertos agora</h3><ul>${p.sockets.map(c => `<li>${esc(c.local)}:${c.localPort}</li>`).join('') || '<li>Nenhum nesta amostra.</li>'}</ul>
    <p>Um socket UDP aberto não comprova transferência. Os destinos UDP aparecem acima quando há eventos de tráfego. IPs privados e de loopback também são contabilizados.</p>
    <p>Um IP pode atender vários serviços. CPU/GPU elevada também pode ter causas legítimas. Conteúdo criptografado não é lido.</p>`;
  if(!p.archived) { const button=document.createElement('button'); button.textContent='Encerrar este processo'; button.className='danger'; button.onclick=()=>requestOperation('terminate',{pid:p.pid,startedTicks:p.startedTicks}); $('detailBody').appendChild(button); } $('detail').showModal();
}
function showCoverage(data) {
  const n = data.network || {};
  const warnings = [...data.warnings];
  if (!n.running) warnings.push(n.error ? (/2147024891|Acesso negado/.test(n.error) ? 'Ative a medição por programa com permissão de administrador.' : 'Não foi possível iniciar a medição por programa. Os detalhes técnicos ficam na exportação da sessão.') : 'Iniciando coleta de tráfego por programa…');
  if (n.parseErrors) warnings.push(`${n.parseErrors} eventos não puderam ser interpretados; totais podem estar incompletos.`);
  if (n.eventsLost || n.realtimeBuffersLost) warnings.push(`Perda ETW: ${n.eventsLost || 0} eventos e ${n.realtimeBuffersLost || 0} buffers. Totais incompletos.`);
  if (n.statsError) warnings.push('Não foi possível verificar perda de eventos ETW.');
  if (n.evictedFlows) warnings.push('Alguns fluxos saíram da lista de detalhes por limite de memória; os totais por programa foram preservados.');
  if (n.omittedEvents) warnings.push(`${n.omittedEvents} eventos omitidos por limite de memória; totais incompletos.`);
  $('coverage').textContent = warnings.length ? warnings.join(' ') : `TCP e UDP por processo ativos · ${n.events.toLocaleString('pt-BR')} eventos · GPU disponível · ${n.eventsLost ?? '—'} eventos perdidos`;
  $('elevate').hidden = !!n.running || !n.error;
}
function updateSignals(data) {
  const active = new Set(); const high = [];
  for (const p of apps.filter(p => !p.archived)) {
    active.add(p.key);
    const elevated = p.cpuPercent >= 20 || p.gpu >= 50;
    if (!elevated) { sustained.delete(p.key); continue; }
    const since = sustained.get(p.key) ?? data.time; sustained.set(p.key, since);
    if (data.time - since >= 20000) high.push(`${p.name} (CPU ${pct(p.cpuPercent)}, GPU ${pct(p.gpu)})`);
  }
  for (const key of sustained.keys()) if (!active.has(key)) sustained.delete(key);
  $('signalsText').textContent = high.length ? `${high.join('; ')}: uso elevado persistente nas amostras por pelo menos 20 segundos. Investigue se corresponde ao que você está fazendo. Isso não confirma mineração.` :
    'Nenhum uso elevado persistente detectado nas amostras (CPU ≥ 20% ou GPU ≥ 50%, por pelo menos 20 s). Isso não descarta malware; confira também os limites de coleta acima.';
}
async function poll() {
  if (paused || busy) return; busy = true; const runGeneration = generation;
  try {
    if (!window.__TAURI__) throw Error('Abra o aplicativo desktop para acessar a coleta real do Windows.');
    const data = await window.__TAURI__.core.invoke('snapshot');
    if (paused || runGeneration !== generation) return;
    current = data; apps = derive(data, previous);
    if (previous && data.netAvailable && previous.netAvailable) {
      const dt = (data.time - previous.time) / 1000; let down = 0, up = 0;
      const old = new Map(previous.adapters.map(a => [a.name, a]));
      for (const a of data.adapters) { const b = old.get(a.name); if (b) { down += Math.max(0, a.received - b.received); up += Math.max(0, a.sent - b.sent); } }
      if (dt > 0) {
        totalDown += down; totalUp += up; history.push({ time: data.time, down: down / dt, up: up / dt }); history = history.slice(-60);
        $('down').textContent = fmt(down / dt) + '/s'; $('up').textContent = fmt(up / dt) + '/s'; draw();
        $('volume').textContent = `Interfaces nesta sessão: ↓ ${fmt(totalDown)} · ↑ ${fmt(totalUp)}`;
      }
    }
    if (!data.netAvailable) { $('down').textContent = 'Indisponível'; $('up').textContent = 'Indisponível'; }
    sessionStart ??= data.time; previous = data;
    $('connections').textContent = `${data.tcpAvailable ? data.connections.length : '—'} / ${data.udpAvailable ? data.udp.length : '—'}`;
    $('count').textContent = apps.filter(p=>(p.rates[''].sent||0)+(p.rates[''].received||0)>0).length;
    updateSignals(data); showCoverage(data);
    $('status').textContent = `Coleta local · ${new Date(data.time).toLocaleTimeString('pt-BR')} · ${data.adapters.length} interfaces ativas`;
    render();
  } catch (e) {
    if (paused || runGeneration !== generation) return;
    $('status').textContent = 'Falha na coleta: ' + String(e); previous = undefined; sustained.clear();
    $('down').textContent = '—'; $('up').textContent = '—';
    $('signalsText').textContent = 'Dados indisponíveis. Não é possível avaliar sinais nesta amostra.';
  } finally { busy = false; }
}
$('rows').addEventListener('click', e => { const b = e.target.closest('[data-key]'); if (b) details(b.dataset.key); });
$('search').addEventListener('input', render);
$('protocol').addEventListener('change', render); $('trafficMode').addEventListener('change', render); $('sort').addEventListener('change', render);
$('close').onclick = () => $('detail').close();
$('filter').onclick = () => { all = !all; $('filter').setAttribute('aria-pressed', String(all)); render(); };
$('pause').onclick = async () => {
  const next = !paused; const button = $('pause'); button.disabled = true;
  try {
    await window.__TAURI__.core.invoke('set_capture_paused', { paused: next });
    paused = next; generation++; sustained.clear(); button.textContent = paused ? 'Retomar' : 'Pausar';
    previous = undefined;
    if (paused) $('status').textContent = 'Pausado · exibindo a última amostra · eventos de tráfego não são acumulados durante a pausa';
    else poll();
  } catch (e) { $('status').textContent = 'Não foi possível alterar a coleta: ' + String(e); }
  finally { button.disabled = false; }
};
$('elevate').onclick = async () => {
  try { await window.__TAURI__.core.invoke('restart_elevated'); }
  catch (e) { $('coverage').textContent = String(e); }
};
$('export').onclick = () => {
  if (!current) { $('status').textContent = 'Aguarde a primeira coleta antes de exportar.'; return; }
  const payload = { app: 'Clareia 0.3', sessionStart, exportedAt: new Date().toISOString(), scope: 'Metadados ETW TCP/UDP IPv4/IPv6, GPU por motor mais ocupado, CPU e interfaces; inclui rede local e loopback', totals: { received: totalDown, sent: totalUp }, history, snapshot: current };
  const url = URL.createObjectURL(new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' }));
  const a = document.createElement('a'); a.href = url; a.download = 'clareia-sessao.json'; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
};
poll(); setInterval(poll, 5000);






# Helpers for the README screenshots. Source this; needs `pnpm preview:ui` on :1430.
B="http://localhost:1430/preview.html"
ab() { agent-browser --session sbprev "$@" >/dev/null 2>&1; }
abq() { agent-browser --session sbprev "$@" 2>/dev/null | tail -1; }
# A Max account with every limit reporting, so the popover shows all it can.
INJECT='(async()=>{const m=await import("/src/lib/store.ts");const now=Date.now();const iso=h=>new Date(now+h*3600e3).toISOString();const snap={five_hour:{utilization:38,resets_at:iso(2.25)},seven_day:{utilization:46,resets_at:iso(51)},seven_day_sonnet:{utilization:12,resets_at:iso(51)},seven_day_opus:{utilization:31,resets_at:iso(51)},seven_day_fable:{utilization:21,resets_at:iso(80)},extra_usage:{is_enabled:true,monthly_limit_cents:5000,used_credits_cents:900,utilization:18,resets_at:null},fetched_at:new Date(now-60000).toISOString()};const s=m.useAppStore.getState();const usage={...s.usage,snapshot:snap,burn_rate:{utilization_per_min:0.15,projected_at_reset:58}};m.useAppStore.setState({usage,accounts:s.accounts.map(a=>a.is_active?{...a,subscription_type:"max",cached_usage:usage}:a)});return "ok"})()'
# clk <regex> [n]: click the n-th control whose aria-label (or, without one, its text) matches.
clk() { abq eval "(()=>{const re=new RegExp('$1','i');const b=[...document.querySelectorAll('button,[role=button],[role=radio],[role=switch],[role=tab],[role=option],label,summary')].filter(b=>re.test(b.getAttribute('aria-label')||b.textContent.trim()));const t=b[${2:-0}];if(!t)return 'NONE for $1 ('+b.length+')';t.click();return 'clicked $1 ('+b.length+')'})()"; }
# ovr '<json object: command → answer>': later IPC calls for those commands get these answers.
ovr() { abq eval "(()=>{const t=window.__TAURI_INTERNALS__;window.__ovr=Object.assign(window.__ovr||{},$1);if(!t.__wrapped){const o=t.invoke;t.invoke=(c,a,...r)=>c in window.__ovr?Promise.resolve(window.__ovr[c]):o(c,a,...r);t.__wrapped=true}return 'overrides: '+Object.keys(window.__ovr).join(',')})()"; }
fresh() { ab open "$B?$1"; abq eval "localStorage.clear()" >/dev/null; ab open "$B?$1"; ab wait 1000; }

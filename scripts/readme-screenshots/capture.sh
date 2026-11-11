#!/bin/zsh
# Retakes the README screenshots from the UI preview (sample data only).
# Needs `pnpm preview:ui` running on :1430 and the agent-browser CLI.
# Raw captures land in $RAW; run finish.sh next.
R=${0:A:h}; source $R/lib.sh; RAW=${RAW:-${TMPDIR:-/tmp}/switchboard-readme-shots}; mkdir -p $RAW
# The signed-in sample account: a Max plan with every limit reporting.
INJECT=${INJECT//riley.chen/riley}
INJECT=${INJECT/const usage=\{...s.usage,/const usage=\{...s.usage,account_email:\"riley@example.com\",}
INJECT=${INJECT/subscription_type:\"max\",/subscription_type:\"max\",email:\"riley@example.com\",}
# Pulsing dots are caught mid-fade in a still, so hold them at rest.
STILL="(()=>{const s=document.createElement('style');s.textContent='*{animation:none!important}';document.head.append(s);return 'still'})()"
# The glance view before the app has enough samples to project a burn rate.
GLANCE=${INJECT/burn_rate:\{utilization_per_min:0.15,projected_at_reset:58\}/burn_rate:null}
shot() { ab wait ${2:-700}; ab screenshot $RAW/$1.png; echo "  $1"; }
tab() { clk "^$1\$" >/dev/null; ab wait 1100; }
frame() { abq eval "document.getElementById('frame').style.height='$1px'" >/dev/null; ab set viewport 360 $1 2; }

echo expanded
ab set viewport 960 640 2
fresh "scale=demo&theme=light"; abq eval "$INJECT" >/dev/null
shot sessions
ovr '{"vscode_tab_available":true}' >/dev/null; tab Cost; shot cost
tab Sessions; clk '^Recap only$' >/dev/null; ab wait 500; abq eval "[...document.querySelectorAll('button[aria-expanded=false]')].filter(b=>/turns/.test(b.textContent))[2].click()" >/dev/null; shot recap 900; clk '^Recap only$' >/dev/null
tab Trends; shot trends
clk '^[A-Z][a-z]{2} [0-9]+: ' 21 >/dev/null; shot trends-day 900
tab Breakdown; shot breakdown
ovr '{"list_providers":[{"id":"official","name":"Anthropic","kind":"official","base_url":null,"auth_token":null,"env":{},"extra_args":[],"preset_id":null,"sort_index":0},{"id":"glm","name":"GLM (z.ai)","kind":"third_party","base_url":"https://api.z.ai/api/anthropic","auth_token":"example-token","env":{"ANTHROPIC_MODEL":"glm-5.3"},"extra_args":[],"preset_id":null,"sort_index":1},{"id":"kimi","name":"Kimi","kind":"third_party","base_url":"https://api.kimi.com/coding","auth_token":"example-token","env":{"ANTHROPIC_MODEL":"kimi-k3.1"},"extra_args":[],"preset_id":null,"sort_index":2},{"id":"relay","name":"Example relay","kind":"third_party","base_url":"https://relay.example.com","auth_token":"example-token","env":{"ANTHROPIC_MODEL":"deepseek-v4.1"},"extra_args":[],"preset_id":null,"sort_index":3}]}' >/dev/null
tab Providers; shot providers
tab Trends; clk '^Settings$' >/dev/null; shot settings 900
abq eval "(()=>{const h=[...document.querySelectorAll('div,span,p,h2,h3')].find(e=>e.children.length===0&&/^account rotation\$/i.test(e.textContent.trim()));h.scrollIntoView({block:'start'});const sw=h.parentElement.querySelector('[role=switch]')||h.parentElement.parentElement.querySelector('[role=switch]');return sw?(sw.click(),'on'):'no switch'})()"
shot rotation 900

# The accounts list is cropped from the sidebar of a taller window, so all four rows fit.
ab set viewport 960 720 2; abq eval "document.getElementById('frame').style.height='720px'" >/dev/null; shot accounts-tall 600
ab set viewport 960 640 2

echo themes
for p in terracotta graphite tide moss plum; do ab open "$B?scale=demo&theme=light&palette=$p"; ab wait 900; abq eval "$INJECT" >/dev/null; shot theme-$p 900; done
ab open "$B?scale=demo&theme=dark"; ab wait 900; abq eval "$INJECT" >/dev/null; shot dark-trends 900
tab Breakdown; shot dark-breakdown
tab Sessions; shot dark-sessions

echo compact
ab set viewport 360 380 2
fresh "scale=demo&theme=light&view=compact"; abq eval "$STILL" >/dev/null; abq eval "$GLANCE" >/dev/null; ab wait 400
frame 208; shot popover
abq eval "$INJECT" >/dev/null
abq eval "JSON.stringify([...document.querySelectorAll('#root > * > *')].map(e=>[Math.round(e.getBoundingClientRect().height),e.scrollHeight]))"
frame 380; clk '^details$' >/dev/null; shot popover-details 900
clk '^details$' >/dev/null
ovr '{"get_warmup_state":{"warmup_enabled":true,"schedule":{"type":"Every5h","anchor":{"hour":8,"minute":0}},"last_warmup_at":null},"get_warmup_consent_granted":true}' >/dev/null
clk '^riley@example' >/dev/null; shot accounts 900
clk '^Switch to morgan' >/dev/null; shot switch-confirm 900
clk '^cancel$' >/dev/null; ab wait 400
clk '^WARM-UP' 1 >/dev/null; ab wait 500
abq eval "(()=>{const b=[...document.querySelectorAll('button[aria-expanded=true]')].find(b=>/warm-up/i.test(b.textContent));if(!b)return 'no row';b.scrollIntoView({block:'center'});return 'ok'})()"
shot warmup 900
echo done

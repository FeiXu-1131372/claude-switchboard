import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  BridgeState,
  BridgeTargetStatus,
  PreflightReport,
} from '../../lib/generated/bindings';

const ipcMock = vi.hoisted(() => ({
  getStatuslineBridgeStatus: vi.fn(),
  preflightStatuslineBridge: vi.fn(),
  enableStatuslineBridge: vi.fn(),
  disableStatuslineBridge: vi.fn(),
  rewrapStatuslineBridge: vi.fn(),
  restoreOriginalStatusline: vi.fn(),
  recheckStatuslineBridge: vi.fn(),
}));
vi.mock('../../lib/ipc', () => ({ ipc: ipcMock }));

import { StatuslineBridgeCard, formatAgo } from '../StatuslineBridgeCard';

const NOW = 1_800_000_000_000;

const userEvent = {
  click: async (el: Element) => {
    fireEvent.click(el);
  },
};
const SETTINGS = '/Users/me/.claude/settings.json';
const BRIDGE = '/Users/me/Library/Application Support/switchboard/bin/switchboard-statusline';

function target(state: BridgeState, over: Partial<BridgeTargetStatus> = {}): BridgeTargetStatus {
  const off = state.kind === 'off' || state.kind === 'claude_code_not_set_up' || state.kind === 'edit_refused';
  return {
    id: 'abc123def456',
    config_home: '/Users/me/.claude',
    settings_path: SETTINGS,
    bridge_path: BRIDGE,
    installed_command: off ? null : `'${BRIDGE}' --target abc123def456`,
    original_command: null,
    shared_warning: null,
    state,
    ...over,
  };
}

function report(over: Partial<PreflightReport> = {}): PreflightReport {
  return {
    command: `'${BRIDGE}' --target abc123def456`,
    shells: [{ shell: '/bin/sh', ok: true, detail: '' }],
    latency_ms: 20,
    settings_path: SETTINGS,
    bridge_path: BRIDGE,
    original_command: 'bash ~/.claude/statusline.sh',
    original_direct: 'ok',
    original_via_bridge: 'ok',
    shared_warning: null,
    ...over,
  };
}

function renderCard(t: BridgeTargetStatus | BridgeTargetStatus[]) {
  ipcMock.getStatuslineBridgeStatus.mockResolvedValue(Array.isArray(t) ? t : [t]);
  return render(<StatuslineBridgeCard now={() => NOW} />);
}

beforeEach(() => {
  Object.values(ipcMock).forEach((fn) => fn.mockReset());
});

describe('formatAgo', () => {
  it.each([
    [0, '0s ago'],
    [12_000, '12s ago'],
    [4 * 60_000, '4m ago'],
    [2 * 3_600_000, '2h ago'],
    [3 * 86_400_000, '3d ago'],
    [-5_000, '0s ago'],
  ])('%i ms before now reads %s', (ms, text) => {
    expect(formatAgo(NOW - ms, NOW)).toBe(text);
  });
});

describe('StatuslineBridgeCard states (spec §7)', () => {
  const cases: Array<[string, BridgeState, RegExp, boolean]> = [
    ['off', { kind: 'off' }, /live usage from claude code/i, false],
    ['waiting', { kind: 'waiting' }, /start or continue a claude code session/i, true],
    ['live', { kind: 'live', last_update_ms: NOW - 12_000 }, /last update 12s ago/i, true],
    ['quiet', { kind: 'quiet', last_update_ms: NOW - 2 * 3_600_000 }, /last update 2h ago/i, true],
    [
      'running_without_usage',
      { kind: 'running_without_usage', reason: 'routed through a custom endpoint' },
      /running the bridge but isn't sending usage: routed through a custom endpoint/i,
      true,
    ],
    [
      'blocked',
      { kind: 'blocked', reason: "Claude Code's settings turn off status line commands (disableAllHooks)." },
      /disableAllHooks/,
      true,
    ],
    [
      'broken',
      { kind: 'broken', error: 'exit 127' },
      /your status line isn't working: the bridge can't run \(exit 127\)/i,
      true,
    ],
    ['changed_outside', { kind: 'changed_outside' }, /changed outside switchboard, so live usage is off/i, false],
    [
      'claude_code_not_set_up',
      { kind: 'claude_code_not_set_up' },
      /claude code isn't set up on this computer/i,
      false,
    ],
    [
      'edit_refused',
      { kind: 'edit_refused', message: "settings.json isn't valid JSON. Fix it, then try again." },
      /isn't valid JSON/,
      false,
    ],
  ];

  it.each(cases)('renders_every_state_in_section_7: %s', async (_name, state, text, toggleOn) => {
    renderCard(target(state));
    expect(await screen.findByText(text)).toBeInTheDocument();
    const toggle = screen.getByRole('switch', { name: /live usage from claude code/i });
    if (toggleOn) expect(toggle).toBeChecked();
    else expect(toggle).not.toBeChecked();
  });

  it('shows the Live badge only for live and the Waiting badge only for waiting', async () => {
    const { unmount } = renderCard(target({ kind: 'live', last_update_ms: NOW - 1000 }));
    expect(await screen.findByText('Live')).toBeInTheDocument();
    expect(screen.queryByText('Waiting')).toBeNull();
    unmount();
    renderCard(target({ kind: 'waiting' }));
    expect(await screen.findByText('Waiting')).toBeInTheDocument();
    expect(screen.queryByText('Live')).toBeNull();
  });

  it('quiet is not a warning: no badge', async () => {
    renderCard(target({ kind: 'quiet', last_update_ms: NOW - 120_000 }));
    await screen.findByText(/last update 2m ago/i);
    expect(screen.queryByText('Live')).toBeNull();
    expect(screen.queryByText('Waiting')).toBeNull();
  });

  it('disables the toggle when Claude Code is not set up', async () => {
    renderCard(target({ kind: 'claude_code_not_set_up' }));
    await screen.findByText(/isn't set up on this computer/i);
    expect(screen.getByRole('switch')).toBeDisabled();
  });

  it('lists what does not report in a disclosure while waiting', async () => {
    renderCard(target({ kind: 'waiting' }));
    await screen.findByText('Waiting');
    expect(screen.getByText(/what doesn't report/i)).toBeInTheDocument();
    expect(screen.getByText(/WSL or a remote host/i)).toBeInTheDocument();
  });

  it('shows the config folder only when there is more than one target', async () => {
    renderCard([target({ kind: 'off' })]);
    await screen.findByRole('switch');
    expect(screen.queryByText('/Users/me/.claude')).toBeNull();
  });

  it('shows each config folder when there are several', async () => {
    renderCard([
      target({ kind: 'off' }),
      target({ kind: 'off' }, { id: 'ffffffffffff', config_home: '/Users/me/work/.claude' }),
    ]);
    expect(await screen.findByText('/Users/me/.claude')).toBeInTheDocument();
    expect(screen.getByText('/Users/me/work/.claude')).toBeInTheDocument();
  });

  it('shows the error when the status cannot be read, and Retry reloads it', async () => {
    ipcMock.getStatuslineBridgeStatus.mockRejectedValueOnce(new Error('store is locked'));
    render(<StatuslineBridgeCard now={() => NOW} />);
    expect(await screen.findByText(/couldn't read the status line setup: store is locked/i)).toBeInTheDocument();
    ipcMock.getStatuslineBridgeStatus.mockResolvedValue([target({ kind: 'off' })]);
    await userEvent.click(screen.getByRole('button', { name: /retry/i }));
    expect(await screen.findByRole('switch')).toBeInTheDocument();
    expect(screen.queryByText(/couldn't read/i)).toBeNull();
  });
});

describe('consent panel', () => {
  async function openConsent(r: PreflightReport) {
    ipcMock.preflightStatuslineBridge.mockResolvedValue(r);
    renderCard(target({ kind: 'off' }));
    await userEvent.click(await screen.findByRole('switch'));
    return screen.findByRole('group', { name: /turn on live usage/i });
  }

  it('consent_panel_shows_the_exact_command_and_real_paths', async () => {
    const panel = await openConsent(report());
    expect(ipcMock.preflightStatuslineBridge).toHaveBeenCalledWith('/Users/me/.claude');
    expect(within(panel).getByText(SETTINGS)).toBeInTheDocument();
    expect(within(panel).getByText(`'${BRIDGE}' --target abc123def456`)).toBeInTheDocument();
    expect(within(panel).getByText(`${BRIDGE} --restore --all`)).toBeInTheDocument();
    expect(within(panel).getByText('bash ~/.claude/statusline.sh')).toBeInTheDocument();
    expect(within(panel).getByText(/runs your status line once with sample data/i)).toBeInTheDocument();
    expect(within(panel).getByText(/nothing leaves your computer/i)).toBeInTheDocument();
  });

  it('consent_panel_shows_the_footer_hint_line_only_when_there_is_no_original', async () => {
    const without = await openConsent(report({ original_command: null }));
    expect(within(without).getByText(/hide its footer hint/i)).toBeInTheDocument();
    expect(within(without).queryByText(/keeps working/i)).toBeNull();
  });

  it('does not show the footer hint line when there is an original', async () => {
    const panel = await openConsent(report());
    expect(within(panel).queryByText(/footer hint/i)).toBeNull();
    expect(within(panel).getByText(/keeps working/i)).toBeInTheDocument();
  });

  it('consent_panel_shows_the_latency_line_above_150ms', async () => {
    const slow = await openConsent(report({ latency_ms: 480 }));
    expect(within(slow).getByText(/about 0\.5 s slower/i)).toBeInTheDocument();
  });

  it('does not show the latency line at or below 150ms', async () => {
    const panel = await openConsent(report({ latency_ms: 150 }));
    expect(within(panel).queryByText(/slower/i)).toBeNull();
  });

  it('shows both outputs when the original differs through the bridge', async () => {
    const panel = await openConsent(report({ original_direct: 'direct 12%', original_via_bridge: 'bridged 12%' }));
    expect(within(panel).getByText(/output differed/i)).toBeInTheDocument();
    expect(within(panel).getByText('direct 12%')).toBeInTheDocument();
    expect(within(panel).getByText('bridged 12%')).toBeInTheDocument();
  });

  it('does not show the differs disclosure when outputs match', async () => {
    const panel = await openConsent(report());
    expect(within(panel).queryByText(/output differed/i)).toBeNull();
  });

  it('shared_file_requires_the_checkbox_before_turn_on_is_enabled', async () => {
    const panel = await openConsent(report({ shared_warning: 'This file is synced between computers.' }));
    const turnOn = within(panel).getByRole('button', { name: 'Turn on' });
    expect(turnOn).toBeDisabled();
    await userEvent.click(within(panel).getByRole('checkbox'));
    expect(turnOn).toBeEnabled();
    ipcMock.enableStatuslineBridge.mockResolvedValue(target({ kind: 'waiting' }));
    await userEvent.click(turnOn);
    await waitFor(() =>
      expect(ipcMock.enableStatuslineBridge).toHaveBeenCalledWith('/Users/me/.claude', true),
    );
  });

  it('sends accepted=false when there is no shared-file warning', async () => {
    const panel = await openConsent(report());
    ipcMock.enableStatuslineBridge.mockResolvedValue(target({ kind: 'waiting' }));
    await userEvent.click(within(panel).getByRole('button', { name: 'Turn on' }));
    await waitFor(() =>
      expect(ipcMock.enableStatuslineBridge).toHaveBeenCalledWith('/Users/me/.claude', false),
    );
    expect(await screen.findByText('Waiting')).toBeInTheDocument();
    expect(screen.getByText('Live usage is on.')).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: /turn on live usage/i })).toBeNull();
  });

  it('blocks Turn on and shows the reason when the bridge failed in a shell', async () => {
    const panel = await openConsent(
      report({ shells: [{ shell: '/bin/sh', ok: false, detail: 'exit 127' }] }),
    );
    expect(within(panel).getByText(/couldn't run in \/bin\/sh: exit 127/i)).toBeInTheDocument();
    expect(within(panel).getByRole('button', { name: 'Turn on' })).toBeDisabled();
  });

  it('cancel_writes_nothing', async () => {
    const panel = await openConsent(report());
    await userEvent.click(within(panel).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('group', { name: /turn on live usage/i })).toBeNull();
    expect(ipcMock.enableStatuslineBridge).not.toHaveBeenCalled();
    expect(ipcMock.disableStatuslineBridge).not.toHaveBeenCalled();
    expect(screen.getByRole('switch')).not.toBeChecked();
  });

  it('an_ipc_rejection_shows_the_error_text (preflight)', async () => {
    ipcMock.preflightStatuslineBridge.mockRejectedValue(new Error('settings.json is read-only.'));
    renderCard(target({ kind: 'off' }));
    await userEvent.click(await screen.findByRole('switch'));
    expect(await screen.findByRole('alert')).toHaveTextContent('settings.json is read-only.');
    expect(screen.queryByRole('group', { name: /turn on live usage/i })).toBeNull();
    expect(screen.getByRole('switch')).toBeEnabled();
  });

  it('an_ipc_rejection_shows_the_error_text (enable)', async () => {
    const panel = await openConsent(report());
    ipcMock.enableStatuslineBridge.mockRejectedValue(
      new Error('settings.json changed while Switchboard was editing it. Try again.'),
    );
    await userEvent.click(within(panel).getByRole('button', { name: 'Turn on' }));
    expect(await screen.findByText(/changed while Switchboard was editing it/i)).toBeInTheDocument();
    // The panel stays open so the user can try again.
    expect(screen.getByRole('group', { name: /turn on live usage/i })).toBeInTheDocument();
  });
});

describe('turning off and the repair buttons', () => {
  it('turns off through the toggle and reloads the status', async () => {
    renderCard(target({ kind: 'quiet', last_update_ms: NOW - 60_000 }));
    ipcMock.disableStatuslineBridge.mockResolvedValue('restored');
    await userEvent.click(await screen.findByRole('switch'));
    await waitFor(() => expect(ipcMock.disableStatuslineBridge).toHaveBeenCalledWith('abc123def456'));
    expect(await screen.findByText(/your status line is back exactly as it was/i)).toBeInTheDocument();
    expect(ipcMock.getStatuslineBridgeStatus).toHaveBeenCalledTimes(2);
  });

  it('says so when the status line was changed since and was left alone', async () => {
    renderCard(target({ kind: 'quiet', last_update_ms: NOW - 60_000 }));
    ipcMock.disableStatuslineBridge.mockResolvedValue('released_user_changed');
    await userEvent.click(await screen.findByRole('switch'));
    expect(await screen.findByText(/left it as it is/i)).toBeInTheDocument();
  });

  it('shows the error when turning off fails', async () => {
    renderCard(target({ kind: 'quiet', last_update_ms: NOW - 60_000 }));
    ipcMock.disableStatuslineBridge.mockRejectedValue(new Error('settings.json is read-only.'));
    await userEvent.click(await screen.findByRole('switch'));
    expect(await screen.findByRole('alert')).toHaveTextContent('settings.json is read-only.');
  });

  it('changed_outside_offers_use_the_bridge_again_and_dismiss', async () => {
    renderCard(target({ kind: 'changed_outside' }));
    const again = await screen.findByRole('button', { name: 'Use the bridge again' });
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeInTheDocument();

    ipcMock.rewrapStatuslineBridge.mockResolvedValue(target({ kind: 'waiting' }));
    await userEvent.click(again);
    await waitFor(() => expect(ipcMock.rewrapStatuslineBridge).toHaveBeenCalledWith('abc123def456'));
    expect(await screen.findByText('Waiting')).toBeInTheDocument();
  });

  it('dismiss releases the target without restoring anything', async () => {
    renderCard(target({ kind: 'changed_outside' }));
    ipcMock.disableStatuslineBridge.mockResolvedValue('released_user_changed');
    await userEvent.click(await screen.findByRole('button', { name: 'Dismiss' }));
    await waitFor(() => expect(ipcMock.disableStatuslineBridge).toHaveBeenCalledWith('abc123def456'));
    expect(ipcMock.restoreOriginalStatusline).not.toHaveBeenCalled();
  });

  it('shows the refusal when the new command already contains the bridge', async () => {
    renderCard(target({ kind: 'changed_outside' }));
    ipcMock.rewrapStatuslineBridge.mockRejectedValue(
      new Error("That status line already contains the Switchboard bridge, so it can't be wrapped again."),
    );
    await userEvent.click(await screen.findByRole('button', { name: 'Use the bridge again' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/can't be wrapped again/);
  });

  it('broken_offers_restore_original_and_retry', async () => {
    renderCard(target({ kind: 'broken', error: 'exit 127' }));
    const restore = await screen.findByRole('button', { name: 'Restore original' });
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();

    ipcMock.restoreOriginalStatusline.mockResolvedValue('restored');
    await userEvent.click(restore);
    await waitFor(() => expect(ipcMock.restoreOriginalStatusline).toHaveBeenCalledWith('abc123def456'));
    expect(await screen.findByText(/back exactly as it was/i)).toBeInTheDocument();
  });

  it('retry re-checks the bridge and shows the fresh state', async () => {
    renderCard(target({ kind: 'broken', error: 'exit 127' }));
    ipcMock.recheckStatuslineBridge.mockResolvedValue([target({ kind: 'waiting' })]);
    await userEvent.click(await screen.findByRole('button', { name: 'Retry' }));
    expect(await screen.findByText('Waiting')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Restore original' })).toBeNull();
  });

  it('shows the error when restore fails', async () => {
    renderCard(target({ kind: 'broken', error: 'exit 127' }));
    ipcMock.restoreOriginalStatusline.mockRejectedValue(new Error('settings.json is read-only.'));
    await userEvent.click(await screen.findByRole('button', { name: 'Restore original' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('settings.json is read-only.');
  });

  it('disables the buttons while a request is in flight', async () => {
    renderCard(target({ kind: 'broken', error: 'exit 127' }));
    let release: (v: 'restored') => void = () => {};
    ipcMock.restoreOriginalStatusline.mockReturnValue(
      new Promise<'restored'>((resolve) => {
        release = resolve;
      }),
    );
    await userEvent.click(await screen.findByRole('button', { name: 'Restore original' }));
    expect(await screen.findByText(/restoring your original status line/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Restore original' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeDisabled();
    release('restored');
    await screen.findByText(/back exactly as it was/i);
  });
});

describe('platform', () => {
  const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36';
  const WIN_SETTINGS = 'C:\\Users\\me\\.claude\\settings.json';
  const WIN_BRIDGE = 'C:\\Users\\me\\.claude-switchboard\\statusline\\bridge.exe';
  const WIN_COMMAND = 'C:/Users/me/.claude-switchboard/statusline/bridge.exe --target abc123def456';

  beforeEach(() => {
    vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue(WINDOWS_UA);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  function windowsTarget(state: BridgeState): BridgeTargetStatus {
    return target(state, {
      config_home: 'C:\\Users\\me\\.claude',
      settings_path: WIN_SETTINGS,
      bridge_path: WIN_BRIDGE,
    });
  }

  it('card_is_rendered_on_windows', async () => {
    ipcMock.getStatuslineBridgeStatus.mockResolvedValue([windowsTarget({ kind: 'off' })]);
    render(<StatuslineBridgeCard now={() => NOW} />);
    expect(await screen.findByRole('switch')).toBeInTheDocument();
    expect(screen.getByText(/live usage from claude code/i)).toBeInTheDocument();
    expect(ipcMock.getStatuslineBridgeStatus).toHaveBeenCalled();
  });

  it('consent_panel_shows_the_windows_command_and_paths', async () => {
    ipcMock.getStatuslineBridgeStatus.mockResolvedValue([windowsTarget({ kind: 'off' })]);
    ipcMock.preflightStatuslineBridge.mockResolvedValue(
      report({
        command: WIN_COMMAND,
        settings_path: WIN_SETTINGS,
        bridge_path: WIN_BRIDGE,
        shells: [
          { shell: 'Git Bash', ok: true, detail: '' },
          { shell: 'PowerShell 7', ok: true, detail: '' },
        ],
      }),
    );
    render(<StatuslineBridgeCard now={() => NOW} />);
    await userEvent.click(await screen.findByRole('switch'));
    const panel = await screen.findByRole('group', { name: /turn on live usage/i });
    expect(within(panel).getByText(WIN_SETTINGS)).toBeInTheDocument();
    expect(within(panel).getByText(WIN_COMMAND)).toBeInTheDocument();
    expect(within(panel).getByText(`${WIN_BRIDGE} --restore --all`)).toBeInTheDocument();
    expect(within(panel).getByRole('button', { name: 'Turn on' })).toBeEnabled();
  });

  it('quotes_the_windows_restore_command_when_the_path_has_a_space', async () => {
    const spaced = 'C:\\Users\\Fei Xu\\.claude-switchboard\\statusline\\bridge.exe';
    ipcMock.getStatuslineBridgeStatus.mockResolvedValue([windowsTarget({ kind: 'off' })]);
    ipcMock.preflightStatuslineBridge.mockResolvedValue(
      report({ command: WIN_COMMAND, settings_path: WIN_SETTINGS, bridge_path: spaced }),
    );
    render(<StatuslineBridgeCard now={() => NOW} />);
    await userEvent.click(await screen.findByRole('switch'));
    const panel = await screen.findByRole('group', { name: /turn on live usage/i });
    // PowerShell needs the call operator in front of a quoted path.
    expect(within(panel).getByText(`& "${spaced}" --restore --all`)).toBeInTheDocument();
  });

  it('names_the_windows_shell_that_failed_and_blocks_turn_on', async () => {
    ipcMock.getStatuslineBridgeStatus.mockResolvedValue([windowsTarget({ kind: 'off' })]);
    ipcMock.preflightStatuslineBridge.mockResolvedValue(
      report({
        command: WIN_COMMAND,
        shells: [
          { shell: 'Git Bash', ok: true, detail: '' },
          { shell: 'PowerShell 7', ok: false, detail: 'exit 1: The term is not recognized' },
        ],
      }),
    );
    render(<StatuslineBridgeCard now={() => NOW} />);
    await userEvent.click(await screen.findByRole('switch'));
    const panel = await screen.findByRole('group', { name: /turn on live usage/i });
    expect(
      within(panel).getByText(/couldn't run in PowerShell 7: exit 1: The term is not recognized/i),
    ).toBeInTheDocument();
    expect(within(panel).getByRole('button', { name: 'Turn on' })).toBeDisabled();
  });
});

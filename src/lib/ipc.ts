import { commands, type Result } from './generated/bindings';
import type { Settings } from './types';

async function unwrap<T>(r: Result<T, string>): Promise<T> {
  if (r.status === 'error') throw new Error(r.error);
  return r.data;
}

export const ipc = {
  getCurrentUsage: () => commands.getCurrentUsage().then(unwrap),
  getPricing: () => commands.getPricing().then(unwrap),
  getSessionHistory: (days: number) => commands.getSessionHistory(days).then(unwrap),
  getDailyTrends: (days: number) => commands.getDailyTrends(days).then(unwrap),
  getModelBreakdown: (days: number) => commands.getModelBreakdown(days).then(unwrap),
  getProjectBreakdown: (days: number) => commands.getProjectBreakdown(days).then(unwrap),
  getCacheStats: (days: number) => commands.getCacheStats(days).then(unwrap),

  startOauthFlow: (longLived: boolean = false) =>
    commands.startOauthFlow(longLived).then(unwrap),
  hasClaudeCodeCreds: () => commands.hasClaudeCodeCreds().then(unwrap),

  listAccounts: () => commands.listAccounts().then(unwrap),
  addAccountFromClaudeCode: () => commands.addAccountFromClaudeCode().then(unwrap),
  removeAccount: (slot: number) => commands.removeAccount(slot).then(unwrap),
  swapToAccount: (slot: number) => commands.swapToAccount(slot).then(unwrap),
  detectRunningClaudeCode: () => commands.detectRunningClaudeCode().then(unwrap),
  refreshAccount: (slot: number) => commands.refreshAccount(slot).then(unwrap),

  getSettings: () => commands.getSettings().then(unwrap),
  updateSettings: (s: Settings) => commands.updateSettings(s).then(unwrap),

  resizeWindow: (mode: 'compact' | 'expanded') => commands.resizeWindow(mode).then(unwrap),
  forceRefresh: (scope: 'active' | 'all') => commands.forceRefresh(scope).then(unwrap),
};

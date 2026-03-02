// @vitest-environment jsdom
import { fireEvent, render, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ConfigSelect } from './ConfigSelect';

vi.mock('@lobehub/icons', () => ({
  Claude: { Color: () => <span data-testid="mock-claude-icon" /> },
  Gemini: { Color: () => <span data-testid="mock-gemini-icon" /> },
}));

vi.mock('../../../../../assets/model-icons/openai.svg', () => ({
  default: 'mock-openai-icon.svg',
}));

describe('ConfigSelect usage entry', () => {
  it('shows live usage entry only when provider is codex', async () => {
    const { container, rerender } = render(
      <ConfigSelect
        currentProvider="codex"
        onProviderChange={() => {}}
      />,
    );

    fireEvent.click(container.querySelector('.config-button') as HTMLElement);
    await waitFor(() => {
      expect(container.querySelector('.selector-option-live-usage')).toBeTruthy();
    });

    rerender(
      <ConfigSelect
        currentProvider="claude"
        onProviderChange={() => {}}
      />,
    );

    await waitFor(() => {
      expect(container.querySelector('.selector-option-live-usage')).toBeFalsy();
    });
  });

  it('triggers usage refresh callback from live usage entry', async () => {
    const onRefreshAccountRateLimits = vi.fn().mockResolvedValue(undefined);
    const { container } = render(
      <ConfigSelect
        currentProvider="codex"
        onProviderChange={() => {}}
        onRefreshAccountRateLimits={onRefreshAccountRateLimits}
      />,
    );

    fireEvent.click(container.querySelector('.config-button') as HTMLElement);
    const usageEntry = container.querySelector('.selector-option-live-usage');
    expect(usageEntry).toBeTruthy();

    fireEvent.click(usageEntry as HTMLElement);
    await waitFor(() => {
      expect(onRefreshAccountRateLimits).toHaveBeenCalled();
    });
  });

  it('shows and toggles plan mode switch only for codex', async () => {
    const onSelectCollaborationMode = vi.fn();
    const { container, rerender } = render(
      <ConfigSelect
        currentProvider="codex"
        onProviderChange={() => {}}
        selectedCollaborationModeId="code"
        onSelectCollaborationMode={onSelectCollaborationMode}
      />,
    );

    fireEvent.click(container.querySelector('.config-button') as HTMLElement);
    const planModeRow = container.querySelector('.selector-option-plan-mode');
    expect(planModeRow).toBeTruthy();

    const planSwitch = container.querySelector('.selector-option-plan-mode .ant-switch');
    expect(planSwitch).toBeTruthy();
    fireEvent.click(planSwitch as HTMLElement);
    await waitFor(() => {
      expect(onSelectCollaborationMode).toHaveBeenCalledWith('plan');
    });

    rerender(
      <ConfigSelect
        currentProvider="codex"
        onProviderChange={() => {}}
        selectedCollaborationModeId="code"
      />,
    );

    await waitFor(() => {
      expect(container.querySelector('.selector-option-plan-mode')).toBeTruthy();
      expect(
        container.querySelector('.selector-option-plan-mode .ant-switch-disabled'),
      ).toBeTruthy();
    });

    rerender(
      <ConfigSelect
        currentProvider="claude"
        onProviderChange={() => {}}
        selectedCollaborationModeId="code"
      />,
    );

    await waitFor(() => {
      expect(container.querySelector('.selector-option-plan-mode')).toBeFalsy();
    });
  });

  it('defaults plan mode switch to off when mode is unset', async () => {
    const { container } = render(
      <ConfigSelect
        currentProvider="codex"
        onProviderChange={() => {}}
      />,
    );

    fireEvent.click(container.querySelector('.config-button') as HTMLElement);
    const planSwitch = container.querySelector('.selector-option-plan-mode .ant-switch');
    expect(planSwitch).toBeTruthy();
    expect(planSwitch?.classList.contains('ant-switch-checked')).toBe(false);
  });
});

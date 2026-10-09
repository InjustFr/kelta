import { expect, test } from '@playwright/test';

const HARNESS = '/tests/e2e/settings/harness.html';

test.describe('settings pane', () => {
  test('a key set at the project layer shows the Project badge', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await page.goto(`${HARNESS}?view=settings&section=terminal`);
    const field = page.locator('[data-testid="field"][data-path="terminal.font_size"]');
    await expect(field).toBeVisible();
    await expect(field.locator('[data-testid="source-badge"]')).toHaveAttribute('data-source', 'default');

    // switch to the Project layer
    await page.getByRole('button', { name: 'Project', exact: true }).click();
    await expect(page.locator('[data-testid="settings-section"]')).toHaveAttribute('data-layer', 'project');

    const input = field.getByRole('spinbutton');
    await input.fill('15');
    await input.press('Tab');
    await expect(field.locator('[data-testid="source-badge"]')).toHaveAttribute('data-source', 'project');
    await expect(field.locator('[data-testid="source-badge"]')).toContainText('Project');
    await expect(input).toHaveValue('15');

    // reset at this layer brings the default back
    await field.getByTestId('reset').click();
    await expect(field.locator('[data-testid="source-badge"]')).toHaveAttribute('data-source', 'default');
    expect(errors).toEqual([]);
  });
});

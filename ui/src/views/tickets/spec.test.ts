import { describe, expect, it } from 'vitest';

import { readSpec } from './spec';

describe('readSpec (#400)', () => {
  it('one section per heading, the intro untitled, list sections counted', () => {
    const spec = readSpec(
      '<p>Intro</p><h2>Goal</h2><p>Check a balance.</p><h2>Scope</h2><ul><li>a</li><li>b</li></ul>',
    );
    expect(spec.sections.map((s) => [s.title, s.count])).toEqual([
      ['', null],
      ['Goal', null],
      ['Scope', 2],
    ]);
    expect(spec.sections[1]!.html).toBe('<p>Check a balance.</p>');
    expect(spec.open).toBeNull();
  });

  it('pulls out what is still open', () => {
    const spec = readSpec('<div><h3>Goal</h3><p>x</p><h3>Open questions</h3><p>Rate limit?</p></div>');
    expect(spec.sections.map((s) => s.title)).toEqual(['Goal']);
    expect(spec.open).toMatchObject({ title: 'Open questions', html: '<p>Rate limit?</p>' });
  });

  it('raw ticket links become chips with their label; worded links stay', () => {
    const url = 'https://acme.atlassian.net/browse/SHOP-350';
    const spec = readSpec(
      `<ul><li><a href="${url}">${url}</a></li><li><a href="https://h/o/r/issues/12">https://h/o/r/issues/12</a></li><li><a href="https://x">the doc</a></li></ul>`,
      (href) => (href === url ? { key: 'SHOP-350', title: 'Gift cards epic' } : null),
    );
    const html = spec.sections[0]!.html;
    expect(html).toContain(
      '<a href="' + url + '" class="ref"><span class="ref-key">SHOP-350</span>Gift cards epic</a>',
    );
    expect(html).toContain('<span class="ref-key">#12</span></a>');
    expect(html).toContain('<a href="https://x">the doc</a>');
  });

  it('acceptance criteria as Given / When / Then: lines of one item, or a run of paragraphs', () => {
    const items = readSpec(
      '<ol><li>Zero balance<br><strong>Given</strong> a card at 0<br>When the customer checks<br>Then nothing is applied</li></ol>',
    ).sections[0]!.html;
    expect(items).toBe(
      '<ol><li><strong class="gwt-title">Zero balance</strong><dl class="gwt"><dt>Given</dt><dd>a card at 0</dd><dt>When</dt><dd>the customer checks</dd><dt>Then</dt><dd>nothing is applied</dd></dl></li></ol>',
    );
    const paras = readSpec('<p>Given: a</p><p>when b</p><p>Then c</p><p>Notes</p>').sections[0]!.html;
    expect(paras).toBe(
      '<dl class="gwt"><dt>Given</dt><dd>a</dd><dt>When</dt><dd>b</dd><dt>Then</dt><dd>c</dd></dl><p>Notes</p>',
    );
    // A lone "Given" is prose, not a criterion.
    expect(readSpec('<p>Given the load, we cache.</p>').sections[0]!.html).toBe(
      '<p>Given the load, we cache.</p>',
    );
  });
});

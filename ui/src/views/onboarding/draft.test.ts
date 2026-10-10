import { describe, expect, it } from 'vitest';

import { bindTracker, defaultView } from './draft';

describe('defaultView', () => {
  it('asks for my tickets with who, not provider assignee fields', () => {
    for (const kind of ['jira', 'redmine', 'github', 'gitlab', 'gitea', 'linear']) {
      const v = defaultView(kind, 'KEY');
      expect(v.who, kind).toBe('mine');
      expect(v.assigned_to, kind).toBeNull();
      expect(v.scope, kind).toBeNull();
    }
    expect(defaultView('jira', 'SHOP').jql).toBe('project = SHOP AND statusCategory != Done');
    expect(defaultView('jira', null).jql).not.toMatch(/assignee/);
    expect(defaultView('redmine', '7').project_id).toBe('7');
    expect(defaultView('linear', 'ENG').team).toBe('ENG');
  });

  it('bindTracker starts the binding with that one view', () => {
    const b = bindTracker('gitlab-work', 'gitlab', 'grp/app');
    expect(b.account).toBe('gitlab-work');
    expect(b.views).toMatchObject([{ id: 'mine', project: 'grp/app', who: 'mine' }]);
  });
});

import type { TicketField } from '$lib/gen';

/** A chosen tracker field as shown (#351): `label: value`, or the value alone without a label. */
export const fieldText = (f: TicketField): string => (f.label ? `${f.label}: ${f.value}` : f.value);

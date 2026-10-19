import { format } from 'date-fns'
import { Badge } from '@/components/ui/badge'
import { TableCell, TableRow } from '@/components/ui/table'
import { formatInvoiceAmount } from '@/lib/invoice-utils'
import type { CreditNoteResponse } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'

interface CreditNoteRowProps {
  note: CreditNoteResponse
  track: 'manual' | 'stripe'
  currency: string
  showOperator: boolean
}

const ACTIVE_BADGE_CLASS = 'bg-success/10 text-success border-success/20'
const VOIDED_BADGE_CLASS = 'bg-destructive/10 text-destructive border-destructive/20'

export function CreditNoteRow({ note, track, currency, showOperator }: CreditNoteRowProps) {
  const isVoided = note.status === 'voided'
  const statusLabel = isVoided
    ? m['billing.credit_note_status_voided']()
    : m['billing.credit_note_status_active']()
  const statusClass = isVoided ? VOIDED_BADGE_CLASS : ACTIVE_BADGE_CLASS

  const rowClass = isVoided ? 'line-through' : undefined
  const rowTestId = isVoided ? `credit-note-voided-${note.id}` : `credit-note-row-${note.id}`

  const formattedAmount = formatInvoiceAmount(note.amount, currency)
  const issuedAt = format(new Date(note.createdAt), 'PPP')

  if (track === 'stripe') {
    const numberLabel = note.externalCreditNoteId ?? note.id
    return (
      <TableRow className={rowClass} data-testid={rowTestId}>
        <TableCell className="font-mono">{numberLabel}</TableCell>
        <TableCell>{issuedAt}</TableCell>
        <TableCell>
          <Badge variant="secondary" className={statusClass}>
            {statusLabel}
          </Badge>
        </TableCell>
        <TableCell className="text-right font-mono">{formattedAmount}</TableCell>
      </TableRow>
    )
  }

  return (
    <TableRow className={rowClass} data-testid={rowTestId}>
      <TableCell className="font-mono">{note.id}</TableCell>
      <TableCell>
        <div className="max-w-xs truncate" title={note.memo ?? undefined}>
          {note.memo ?? '—'}
        </div>
      </TableCell>
      {showOperator && <TableCell>{note.createdByUserId ?? '—'}</TableCell>}
      <TableCell>{issuedAt}</TableCell>
      <TableCell>
        <Badge variant="secondary" className={statusClass}>
          {statusLabel}
        </Badge>
      </TableCell>
      <TableCell className="text-right font-mono">{formattedAmount}</TableCell>
    </TableRow>
  )
}

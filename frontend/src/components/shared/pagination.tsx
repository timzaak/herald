import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from '@/components/ui/pagination'
import { m } from '@/paraglide/messages'

const MAX_VISIBLE_PAGES = 7

export interface PaginationProps {
  page: number
  pageSize: number
  total: number
  onPageChange: (page: number) => void
  testIdPrefix?: string
}

export function ListPagination({
  page,
  pageSize,
  total,
  onPageChange,
  testIdPrefix = 'pagination',
}: PaginationProps) {
  const totalPages = pageSize > 0 ? Math.ceil(total / pageSize) : 0

  if (total === 0) return null

  const start = Math.max(0, page - Math.floor(MAX_VISIBLE_PAGES / 2))
  const end = Math.min(totalPages, start + MAX_VISIBLE_PAGES)
  const pageNumbers = Array.from({ length: end - start }, (_, i) => start + i)

  return (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <div className="text-sm text-muted-foreground">
        {m['pagination.showing']({
          from: Math.min(page * pageSize + 1, total),
          to: Math.min((page + 1) * pageSize, total),
          total: total,
        })}
      </div>

      <Pagination data-testid={testIdPrefix}>
        <PaginationContent>
          <PaginationItem>
            <PaginationPrevious
              onClick={() => page > 0 && onPageChange(page - 1)}
              className={page === 0 ? 'opacity-50 pointer-events-none' : undefined}
              data-testid={`${testIdPrefix}-previous`}
            />
          </PaginationItem>

          {pageNumbers.map((pageNum) => (
            <PaginationItem key={pageNum}>
              <PaginationLink
                onClick={() => onPageChange(pageNum)}
                isActive={page === pageNum}
                data-testid={`${testIdPrefix}-page-${pageNum}`}
              >
                {pageNum + 1}
              </PaginationLink>
            </PaginationItem>
          ))}

          <PaginationItem>
            <PaginationNext
              onClick={() => page < totalPages - 1 && onPageChange(page + 1)}
              className={page >= totalPages - 1 ? 'opacity-50 pointer-events-none' : undefined}
              data-testid={`${testIdPrefix}-next`}
            />
          </PaginationItem>
        </PaginationContent>
      </Pagination>
    </div>
  )
}

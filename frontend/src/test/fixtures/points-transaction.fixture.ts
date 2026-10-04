import type { PointsTransactionResponse } from '@/lib/api-generated'

export const mockRechargeTransaction: PointsTransactionResponse = {
  id: 'txn-123',
  walletId: 'acc-123',
  userId: 'user-123',
  realmId: 'realm-123',
  amount: 1000,
  balanceAfter: 6000,
  transactionType: 'recharge',
  description: 'Plan subscription bonus',
  externalRefId: 'sub-123',
  subscriptionId: 'sub-123',
  clientAppId: 'app-123',
  createdAt: '2025-03-15T10:00:00Z',
}

export const mockConsumeTransaction: PointsTransactionResponse = {
  id: 'txn-456',
  walletId: 'acc-123',
  userId: 'user-123',
  realmId: 'realm-123',
  amount: -500,
  balanceAfter: 5500,
  transactionType: 'consume',
  description: 'Service usage',
  externalRefId: 'usage-123',
  clientAppId: 'app-123',
  createdAt: '2025-03-15T11:00:00Z',
}

export const mockTransactionsList: PointsTransactionResponse[] = [
  mockRechargeTransaction,
  mockConsumeTransaction,
  {
    ...mockRechargeTransaction,
    id: 'txn-789',
    amount: 500,
    balanceAfter: 6500,
    description: 'Renewal bonus',
    createdAt: '2025-03-14T10:00:00Z',
  },
  {
    ...mockConsumeTransaction,
    id: 'txn-101',
    amount: -200,
    balanceAfter: 6300,
    description: 'API usage',
    externalRefId: null,
    subscriptionId: null,
    createdAt: '2025-03-14T09:00:00Z',
  },
]

export const mockTransactionsWithoutClientApp: PointsTransactionResponse[] = [
  {
    ...mockRechargeTransaction,
    id: 'txn-202',
    clientAppId: null,
  },
]

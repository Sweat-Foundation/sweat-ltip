import { TransactionResult } from "near-workspaces";

export type Grant = {
  total_amount: string;
  claimed_amount: string;
  order_amount: string;
};

export type Account = {
  grants: Record<number, Grant>;
};

export function sweat(n: number): bigint {
  return BigInt(n) * (10n ** 18n);
}

export function hasError(result: TransactionResult, error: string): boolean {
  return result.failures.findIndex(item => {
    const errorKind = JSON.stringify((item as unknown as Error).ActionError.kind);

    return errorKind.includes(error);
  }) >= 0;
}

type Error = {
  ActionError: {
    index: number,
    kind: object
  }
}

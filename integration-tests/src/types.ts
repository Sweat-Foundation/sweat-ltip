import { NearAccount, TransactionResult } from "near-workspaces";

export type Grant = {
  issued_at: number;
  cliff_end_at: number;
  vesting_end_at: number;
  total_amount: string;
  claimed_amount: string;
  order_amount: string;
  vested_amount: string;
  not_vested_amount: string;
  claimable_amount: string;
  terminated_at: number | undefined;
};

export type Account = {
  account_id: string,
  grants: Grant[],
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

export async function ft_balance(ft: NearAccount, account: NearAccount): Promise<bigint> {
  const balance: string = await ft.view('ft_balance_of', { account_id: account.accountId });

  return BigInt(balance);
}

export const ONE_DAY_IN_SECONDS = 86_400;

export function now(): number {
  return Math.floor(Date.now() / 1000);
}

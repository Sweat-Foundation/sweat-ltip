export type Grant = {
  total_amount: string;
  claimed_amount: string;
  order_amount: string;
};

export type Account = {
  grants: Record<number, Grant>;
};

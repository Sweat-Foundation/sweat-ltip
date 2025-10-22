import { NearAccount, Worker } from 'near-workspaces';
import anyTest, { TestFn } from 'ava'

export type Context = {
  worker: Worker;
  accounts: Record<string, NearAccount>;
}

export function createTest(): TestFn<Context> {
  const test = anyTest as TestFn<Context>;

  test.before(async t => {
    console.log('🍳 Preparing the context');

    const worker = await Worker.init();
    const root = worker.rootAccount;

    const alice = await root.createSubAccount('alice');
    const owner = await root.createSubAccount('owner');
    const issuer = await root.createSubAccount('issuer');
    const executor = await root.createSubAccount('executor');

    const sweat = await prepareFtContract(root, owner);
    const contract = await prepareLtipContract(root, sweat, owner, issuer, executor);

    await storageDeposit(sweat, [contract, alice, issuer, executor]);
    await fundAccounts(sweat, owner, [issuer, alice]);

    t.context.worker = worker;
    t.context.accounts = { root, contract, sweat, alice, owner, issuer, executor };
  });

  test.after(async t => {
    await t.context.worker.tearDown().catch(error => {
      console.log('Failed to tear down the worker:', error);
    });
  });

  return test;
}

async function prepareFtContract(root: NearAccount, owner: NearAccount): Promise<NearAccount> {
  console.log('🚢 Deploy SWEAT contract');
  const ft = await root.devDeploy('../res/sweat.wasm');

  console.log('  ➤ Call ft.new');
  await ft.call(ft, 'new', {});

  console.log('  ➤ Call ft.add_oracle(owner)');
  await ft.call(ft, 'add_oracle', { account_id: owner.accountId });

  return ft;
}

async function prepareLtipContract(root: NearAccount, sweat: NearAccount, owner: NearAccount, issuer: NearAccount, executor: NearAccount): Promise<NearAccount> {
  console.log('🚢 Deploy LTIP contract');
  const contract = await root.devDeploy('../res/sweat_ltip.wasm');

  console.log('  ➤ Call contract.new');
  await contract.call(contract, 'new', {
    token_id: sweat.accountId,
    cliff_duration: 31536000,
    full_unlock_duration: 94608000,
    owner_id: owner.accountId
  });

  console.log('  ➤ Call contract.grant_role');
  await owner.call(contract, 'grant_role', { account_id: issuer.accountId, role: 'issuer' });

  console.log('  ➤ Call contract.grant_role');
  await owner.call(contract, 'grant_role', { account_id: executor.accountId, role: 'executor' });

  return contract;
}

async function storageDeposit(ft: NearAccount, accounts: Array<NearAccount>): Promise<void> {
  console.log('💳 Storage deposit');
  for (var account of accounts) {
    console.log('  ➤ Register', account.accountId);

    await account.call(
      ft,
      'storage_deposit',
      { account_id: account.accountId },
      { attachedDeposit: 2350000000000000000000n }
    );
  }
}

async function fundAccounts(sweat: NearAccount, owner: NearAccount, accounts: Array<NearAccount>): Promise<void> {
  console.log('💸 Fund accounts');
  for (var account of accounts) {
    console.log('  ➤ Funding', account.accountId);

    await sweat.call(
      sweat,
      'tge_mint',
      { account_id: account.accountId, amount: '100000000000000000000000000' }
    );
  }
}

import { captureError, NearAccount, StateItem, Worker } from 'near-workspaces';
import anyTest, { TestFn } from 'ava'
import { readFile, writeFile } from "fs/promises";
import { Account, Contract, Data, RecordBuilder, Records } from 'near-workspaces/dist/record';

export type Context = {
  worker: Worker;
  accounts: Record<string, NearAccount>;
}

export function createProdMirrotTest(): TestFn<Context> {
  const test = anyTest as TestFn<Context>;

  test.before(async t => {
    const worker = await Worker.init();
    const root = worker.rootAccount;

    const contract = await root.importContract({
      mainnetContract: 'ltip.sweat',
      withData: true,
      blockId: 169_690_297,
    });
    await contract.deploy('../res/sweat_ltip.wasm');

    t.context.worker = worker;
    t.context.accounts = { root, contract };
  });

  return test;
}

interface StateDump {
  ft: StateItem[],
  ltip: StateItem[],
}

export async function loadContext(): Promise<Context> {
  console.log('🍳 Loading the context');

  const worker = await Worker.init();
  const root = worker.rootAccount;

  console.log('Create Actor accounts');
  const alice = await root.createSubAccount('alice');
  const bob = await root.createSubAccount('bob');
  const owner = await root.createSubAccount('owner');
  const issuer = await root.createSubAccount('issuer');
  const executor = await root.createSubAccount('executor');

  console.log('Create Contract accounts');
  const ft = await root.createSubAccount('token');
  const contract = await root.createSubAccount('ltip');

  const state: StateDump = JSON.parse(await readFile('state', 'utf-8'));

  await patchContract(ft, '../res/sweat.wasm', state.ft);
  await patchContract(contract, '../res/sweat_ltip.wasm', state.ltip);

  return {
    worker,
    accounts: { root, contract, ft, alice, bob, owner, issuer, executor }
  }
}

export async function prepareContext(cliff_duration?: number | null, full_unlock_duration?: number | null) {
  console.log('🍳 Preparing the context');

  const worker = await Worker.init();
  const root = worker.rootAccount;

  const alice = await root.createSubAccount('alice');
  const bob = await root.createSubAccount('bob');
  const owner = await root.createSubAccount('owner');
  const issuer = await root.createSubAccount('issuer');
  const executor = await root.createSubAccount('executor');

  const ft = await prepareFtContract(root, owner);
  const contract = await prepareLtipContract(root, ft, owner, issuer, executor, cliff_duration, full_unlock_duration);

  await storageDeposit(ft, [contract, alice, bob, issuer, executor]);
  await fundAccounts(ft, [issuer, alice]);

  const [ftState, ltipState] = await Promise.all([
    ft.viewStateRaw(),
    contract.viewStateRaw(),
  ]);

  await writeFile('state', JSON.stringify({ ft: ftState, ltip: ltipState }), 'utf-8');
}

export function createTest(cliff_duration?: number | null, full_unlock_duration?: number | null): TestFn<Context> {
  const test = anyTest as TestFn<Context>;

  test.before(async t => {
    try {
      t.context = await loadContext();
    } catch (err) {
      console.error(err);
    }
  });

  test.after(async t => {
    await t.context.worker.tearDown().catch(error => {
      console.log('Failed to tear down the worker:', error);
    });
  });

  return test;
}

export async function prepareFtContract(root: NearAccount, owner: NearAccount): Promise<NearAccount> {
  console.log('🚢 Deploy SWEAT contract');
  const ft = await root.createSubAccount('token');
  await ft.deploy('../res/sweat.wasm');

  console.log('  ➤ Call ft.new');
  await ft.call(ft, 'new', {});

  console.log('  ➤ Call ft.add_oracle(owner)');
  await ft.call(ft, 'add_oracle', { account_id: owner.accountId });

  return ft;
}

async function prepareLtipContract(
  root: NearAccount,
  ft: NearAccount,
  owner: NearAccount,
  issuer: NearAccount,
  executor: NearAccount,
  cliff_duration?: number | null,
  full_unlock_duration?: number | null
): Promise<NearAccount> {
  console.log('🚢 Deploy LTIP contract');
  const contract = await root.createSubAccount('ltip');
  await contract.deploy('../res/sweat_ltip.wasm');

  console.log('  ➤ Call contract.new');
  await contract.call(contract, 'new', {
    token_id: ft.accountId,
    cliff_duration: cliff_duration ?? 31536000,
    vesting_duration: full_unlock_duration ?? 94608000,
    owner_id: owner.accountId
  });

  console.log('  ➤ Call contract.grant_role(issuer, issuer)');
  await owner.call(contract, 'grant_role', { account_id: issuer.accountId, role: 'issuer' });

  console.log('  ➤ Call contract.grant_role(executor, executor)');
  await owner.call(contract, 'grant_role', { account_id: executor.accountId, role: 'executor' });

  return contract;
}

export async function storageDeposit(ft: NearAccount, accounts: Array<NearAccount>): Promise<void> {
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

export async function fundAccounts(ft: NearAccount, accounts: Array<NearAccount>): Promise<void> {
  console.log('💸 Fund accounts');
  for (var account of accounts) {
    console.log('  ➤ Funding', account.accountId);

    await ft.call(
      ft,
      'tge_mint',
      { account_id: account.accountId, amount: '100000000000000000000000000' }
    );
  }
}

async function patchContract(account: NearAccount, binaryPath: string, state: StateItem[]) {
  console.log('Deploy binary');
  await account.deploy(binaryPath);

  console.log('Load state');
  const dataRecords: Array<Data> = state.map(item => {
    return {
      Data: {
        account_id: account.accountId,
        data_key: item.key,
        value: item.value,
      }
    };
  })

  console.log('Patch ', account.accountId);
  await account.patchStateRecords({
    records: dataRecords
  });
}

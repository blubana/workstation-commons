import asyncio

import pytest

from compute_mgr import npu


def run(coro):
    return asyncio.run(coro)


def test_round_robin_and_single_slot():
    async def main():
        s = npu.Scheduler()
        order = []

        async def job(user, n):
            await s.acquire(user, {"endpoint": "x", "model": "m"})
            assert s.active["user"] == user
            order.append(f"{user}{n}")
            await asyncio.sleep(0.01)
            s.release(True)

        # alice floods first, bob and carol arrive later: they must not wait behind all of alice's jobs
        tasks = [asyncio.create_task(job("alice", i)) for i in range(3)]
        await asyncio.sleep(0)
        tasks += [asyncio.create_task(job("bob", 0)), asyncio.create_task(job("carol", 0))]
        await asyncio.gather(*tasks)
        return order, s

    order, s = run(main())
    assert order[0] == "alice0"
    assert set(order[1:3]) == {"bob0", "carol0"}
    assert s.active is None and s.stats["ok"] == 5


def test_queue_limit(monkeypatch):
    monkeypatch.setattr(npu, "MAX_QUEUED_PER_USER", 2)

    async def main():
        s = npu.Scheduler()
        await s.acquire("alice", {})          # holds the slot
        waiters = [asyncio.create_task(s.acquire("alice", {})) for _ in range(2)]
        await asyncio.sleep(0)
        with pytest.raises(npu.QueueFull):
            await s.acquire("alice", {})
        for _ in range(3):
            s.release(True)
            await asyncio.sleep(0)
        await asyncio.gather(*waiters)

    run(main())


def test_wait_timeout_releases_cleanly(monkeypatch):
    monkeypatch.setattr(npu, "WAIT_TIMEOUT", 0.05)

    async def main():
        s = npu.Scheduler()
        await s.acquire("alice", {})
        with pytest.raises(npu.WaitTimeout):
            await s.acquire("bob", {})
        assert s.queued() == {}
        s.release(True)
        assert s.active is None

    run(main())

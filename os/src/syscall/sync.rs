use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    let mutex: Arc<dyn Mutex> = if blocking {
        Arc::new(MutexBlocking::new())
    } else {
        Arc::new(MutexSpin::new())
    };
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = if let Some(id) = inner.mutex_list.iter().position(Option::is_none) {
        id
    } else {
        inner.mutex_list.push(None);
        inner.mutex_list.len() - 1
    };
    inner.mutex_list[id] = Some(mutex);
    inner.deadlock.register((0, id), 1, blocking);
    id as isize
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let actor = current_actor();
    let process = current_process();
    let mutex = {
        let mut inner = process.inner_exclusive_access();
        let Some(Some(mutex)) = inner.mutex_list.get(mutex_id) else {
            return -1;
        };
        let mutex = Arc::clone(mutex);
        if !inner.deadlock.request((0, mutex_id), actor) {
            return -0xdead;
        }
        mutex
    };
    drop(process);
    mutex.lock();
    current_process()
        .inner_exclusive_access()
        .deadlock
        .acquired((0, mutex_id), actor);
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let actor = current_actor();
    let process = current_process();
    let mutex = {
        let mut inner = process.inner_exclusive_access();
        let Some(Some(mutex)) = inner.mutex_list.get(mutex_id) else {
            return -1;
        };
        let mutex = Arc::clone(mutex);
        if !inner.deadlock.release((0, mutex_id), actor, true) {
            return -1;
        }
        mutex
    };
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    if res_count > isize::MAX as usize {
        return -1;
    }
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = if let Some(id) = inner.semaphore_list.iter().position(Option::is_none) {
        id
    } else {
        inner.semaphore_list.push(None);
        inner.semaphore_list.len() - 1
    };
    inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
    inner.deadlock.register((1, id), res_count, true);
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let actor = current_actor();
    let process = current_process();
    let sem = {
        let mut inner = process.inner_exclusive_access();
        let Some(Some(sem)) = inner.semaphore_list.get(sem_id) else {
            return -1;
        };
        let sem = Arc::clone(sem);
        if sem.inner.exclusive_access().count == isize::MAX {
            return -1;
        }
        if !inner.deadlock.release((1, sem_id), actor, false) {
            return -1;
        }
        sem
    };
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let actor = current_actor();
    let process = current_process();
    let sem = {
        let mut inner = process.inner_exclusive_access();
        let Some(Some(sem)) = inner.semaphore_list.get(sem_id) else {
            return -1;
        };
        let sem = Arc::clone(sem);
        if !inner.deadlock.request((1, sem_id), actor) {
            return -0xdead;
        }
        sem
    };
    drop(process);
    sem.down();
    current_process()
        .inner_exclusive_access()
        .deadlock
        .acquired((1, sem_id), actor);
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    let process = current_process();
    let condvar = {
        let inner = process.inner_exclusive_access();
        let Some(Some(condvar)) = inner.condvar_list.get(condvar_id) else {
            return -1;
        };
        Arc::clone(condvar)
    };
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    let process = current_process();
    let condvar = {
        let inner = process.inner_exclusive_access();
        let Some(Some(condvar)) = inner.condvar_list.get(condvar_id) else {
            return -1;
        };
        Arc::clone(condvar)
    };
    drop(process);
    if sys_mutex_unlock(mutex_id) != 0 {
        return -1;
    }
    // No scheduling occurs between unlock and enqueue in this uniprocessor kernel.
    condvar.wait_unlocked();
    sys_mutex_lock(mutex_id)
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    if enabled > 1 {
        return -1;
    }
    current_process()
        .inner_exclusive_access()
        .deadlock
        .set_enabled(enabled != 0);
    0
}

fn current_actor() -> usize {
    current_task().unwrap().deadlock_id
}

use std::io::{Error, Result};
use std::os::unix::prelude::*;
use std::time::Duration;

pub struct Epoll {
    fd: OwnedFd,
    events: Vec<libc::epoll_event>,
    next_key: u64,
}

pub struct Token {
    fd: RawFd,
    key: u64,
}

macro_rules! syscall {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        let res = unsafe { libc::$fn($($arg, )*) };
        if res == -1 {
            Err(Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}

impl Epoll {
    pub fn new() -> Result<Epoll> {
        let fd = syscall!(epoll_create1(0))?;
        // Only set FD_CLOEXEC if it's supported, ignore it if it's not
        if let Ok(flags) = syscall!(fcntl(fd, libc::F_GETFD)) {
            let _ = syscall!(fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC));
        }

        Ok(Epoll {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            events: Vec::new(),
            next_key: 0,
        })
    }

    pub fn register_read(&mut self, fd: RawFd, once: bool) -> Result<Token> {
        let key = self.next_key;
        self.next_key += 1;
        let mut ev = libc::epoll_event {
            events: (if once { libc::EPOLLONESHOT } else { 0 } | libc::EPOLLIN) as u32,
            u64: key,
        };
        syscall!(epoll_ctl(
            self.fd.as_raw_fd(),
            libc::EPOLL_CTL_ADD,
            fd,
            &mut ev
        ))?;

        Ok(Token { fd, key })
    }

    #[allow(unused)]
    pub fn renew_read(&mut self, token: &Token) -> Result<()> {
        let mut ev = libc::epoll_event {
            events: (libc::EPOLLONESHOT | libc::EPOLLIN) as u32,
            u64: token.key,
        };
        syscall!(epoll_ctl(
            self.fd.as_raw_fd(),
            libc::EPOLL_CTL_MOD,
            token.fd,
            &mut ev
        ))?;
        Ok(())
    }

    #[allow(unused)]
    pub fn unregister(&mut self, token: Token) -> Result<()> {
        syscall!(epoll_ctl(
            self.fd.as_raw_fd(),
            libc::EPOLL_CTL_DEL,
            token.fd,
            std::ptr::null_mut()
        ))?;
        Ok(())
    }

    pub fn wait(&mut self, timeout: Option<Duration>) -> Result<()> {
        self.events.clear();
        self.events.reserve(self.next_key as usize);

        let result = syscall!(epoll_wait(
            self.fd.as_raw_fd(),
            self.events.as_mut_ptr() as *mut libc::epoll_event,
            self.events.capacity() as i32,
            timeout.map(|d| d.as_millis() as i32).unwrap_or(-1),
        ));

        let ready_count = *result.as_ref().unwrap_or(&0);
        unsafe {
            self.events.set_len(ready_count as usize);
        }

        result.map(|_| ())
    }

    #[allow(unused)]
    pub fn test_read(&mut self, token: &Token) -> bool {
        for ev in &self.events {
            if ev.u64 != token.key {
                continue;
            }
            return (ev.events as i32 & libc::EPOLLIN) == libc::EPOLLIN;
        }
        false
    }
}

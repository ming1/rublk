use libublk::io::{UblkDev, UblkQueue};
use libublk::{exe::Executor, UblkError, UblkSession};
use std::rc::Rc;

#[derive(clap::Args, Debug)]
pub struct NullAddArgs {
    #[command(flatten)]
    pub gen_arg: super::args::GenAddArgs,
}

pub fn ublk_add_null(
    sess: UblkSession,
    _id: i32,
    opt: Option<NullAddArgs>,
) -> Result<i32, UblkError> {
    let size = 250_u64 << 30;

    let tgt_init = |dev: &mut UblkDev| {
        dev.set_default_params(size);
        if let Some(ref o) = opt {
            o.gen_arg.apply_block_size(dev);
            o.gen_arg.apply_read_only(dev);
        }
        Ok(0)
    };
    let (mut ctrl, dev) = sess.create_devices(tgt_init).unwrap();
    let depth = dev.dev_info.queue_depth;
    let q_handler = move |qid: u16, dev: &UblkDev| {
        let q_rc = Rc::new(UblkQueue::new(qid as u16, &dev).unwrap());
        let exe = Executor::new(dev.get_nr_ios());

        async fn handle_io(q: &UblkQueue<'_>, tag: u16) -> i32 {
            let iod = q.get_iod(tag);

            (iod.nr_sectors << 9) as i32
        }
        for tag in 0..depth as u16 {
            let q = q_rc.clone();

            exe.spawn(tag as u16, async move {
                let buf_addr = q.get_io_buf_addr(tag);
                let mut cmd_op = libublk::sys::UBLK_IO_FETCH_REQ;
                let mut res = 0;
                loop {
                    let cmd_res = q.submit_io_cmd(tag, cmd_op, buf_addr, res).await;
                    if cmd_res == libublk::sys::UBLK_IO_RES_ABORT {
                        break;
                    }

                    res = handle_io(&q, tag).await;
                    cmd_op = libublk::sys::UBLK_IO_COMMIT_AND_FETCH_REQ;
                }
            });
        }
        q_rc.wait_and_wake_io_tasks(&exe);
    };

    let _shm = {
        if let Some(o) = opt {
            Some(o.gen_arg.get_shm_id())
        } else {
            None
        }
    };
    sess.run_target(&mut ctrl, &dev, q_handler, |dev_id| {
        if let Some(shm) = _shm {
            crate::rublk_write_id_into_shm(&shm, dev_id);
        }
    })
    .unwrap();

    Ok(0)
}

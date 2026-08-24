document.documentElement.lang = "zh-CN";
document.documentElement.style.colorScheme = "light";
document.head.innerHTML = `
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>HAL100 界面无法启动</title>
`;
document.body.innerHTML = `
  <main style="display:grid;min-width:880px;min-height:620px;height:100vh;margin:0;place-items:center;color:#292824;background:#f7f5f0;font-family:-apple-system,BlinkMacSystemFont,'SF Pro Text','Segoe UI','PingFang SC',sans-serif">
    <section role="alert" style="display:grid;grid-template-columns:auto 1fr;width:min(540px,calc(100% - 80px));gap:18px;padding:28px;border:1px solid #e3ded5;border-radius:18px;background:#fffefb;box-shadow:0 18px 50px rgba(55,47,37,.13)">
      <span aria-hidden="true" style="display:grid;width:44px;height:44px;place-items:center;border-radius:13px;color:#fff;background:#b3423e;font-size:20px;font-weight:700">!</span>
      <div>
        <h1 style="margin:1px 0 8px;font-size:20px">HAL100 界面无法启动</h1>
        <p style="margin:0;color:#6e6a63;font-size:13px;line-height:1.7">前端开发服务未运行或不是当前项目。HAL100 已阻止进入空白页面，关闭提示后请使用完整开发入口重新启动。</p>
        <code style="display:block;width:fit-content;margin-top:14px;padding:6px 9px;border-radius:7px;color:#6e6a63;background:#f5f2ec;font-size:12px">pnpm desktop:open</code>
        <small style="display:block;margin-top:12px;color:#918b82">诊断代码：frontend_dev_server_unavailable</small>
      </div>
    </section>
  </main>
`;
document.body.style.margin = "0";

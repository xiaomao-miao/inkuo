/**
 * inkuo Cloud 服务的官方 base URL。所有前端调用 cloudApi
 * 之前都必须从 `getCloudBaseUrl()` 拿,不允许在 UI 上让用户
 * 填写。原因:
 *   1. 跨前端/后端/base URL 是单一可发现的事实源;
 *   2. 防止测试用户被诱导输错地址;
 *   3. 后续切换 staging/production 时只改这一个常量 +
 *      构建环境变量。
 *
 * 临时值:`http://localhost:8080`(本地自托管)
 * TODO: 上线后改为 `https://cloud.inkuo.com`。
 */
const INKUO_CLOUD_BASE_URL = 'http://114.215.182.32:8080';

export function getCloudBaseUrl(): string {
  return INKUO_CLOUD_BASE_URL;
}

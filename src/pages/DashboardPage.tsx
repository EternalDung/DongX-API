export function DashboardPage() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold">仪表盘</h1>
      <p className="mt-2 text-muted-foreground">请求统计、Token 消耗、渠道状态概览</p>
      <div className="mt-6 grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
        {["今日请求", "今日 Token", "活跃渠道", "平均延迟"].map((label) => (
          <div key={label} className="rounded-lg border bg-card p-4">
            <p className="text-sm text-muted-foreground">{label}</p>
            <p className="mt-1 text-2xl font-bold">--</p>
          </div>
        ))}
      </div>
    </div>
  );
}

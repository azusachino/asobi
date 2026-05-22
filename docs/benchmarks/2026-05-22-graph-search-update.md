cargo bench --bench graph
seeded graph: entities=10000, observations=10000
search_nodes selective: total=54.225167ms, avg=1.084503ms, iters=50, hits=11
search_nodes broad capped: total=39.987083ms, avg=7.997416ms, iters=5, hits=100
search_nodes broad export: total=439.095208ms, avg=146.365069ms, iters=3, hits=10000
open_nodes 3 names: total=29.809ms, avg=29.809µs, iters=1000

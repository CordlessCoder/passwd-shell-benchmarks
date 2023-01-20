#!/home/jjs/bin/julia

shellcnt = Dict{String,Int64}()

try
  f = open("passwd", "r")
  while ! eof(f)
    s = readline(f)
    pwline = split(s, ":")
    shell = pwline[7]
    shellcnt["$shell"] = get!(shellcnt, "$shell", 0) +1
#    println("$shell")
  end
  close(f)
catch
    println("file not found")
end

println("\nSummary\n--------------------")
for i in keys(shellcnt)
  println(i, ":\t", shellcnt[i])
end



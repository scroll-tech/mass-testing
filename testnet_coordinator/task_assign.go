package main

import (
	"crypto/sha1"
	"encoding/binary"
	"log"
	"math/rand"
	"sort"
	"sync"
)

type TaskStatus int

const TaskAssigned TaskStatus = 0
const TaskCompleted TaskStatus = 1
const TaskReAssign TaskStatus = 2

const indexUnSet = 18446744073709551615
const samplingBitMask = 1048575 //0xFFFFF

type sampling struct {
	ratioMark uint32
	seedByte  []byte
}

func newSampling(ratio float32) *sampling {

	return &sampling{
		ratioMark: uint32(int64(samplingBitMask) * int64(ratio*10.0) / 1000),
		seedByte:  make([]byte, 8),
	}
}

func (sp *sampling) setSeed(seed uint64) *sampling {
	binary.LittleEndian.PutUint64(sp.seedByte, seed)
	return sp
}

func (sp *sampling) sampleInt(n int64) bool {
	bt := make([]byte, 8)
	binary.BigEndian.PutUint64(bt, uint64(n))
	for i, b := range bt {
		bt[i] = b ^ sp.seedByte[i]
	}
	h := sha1.New()
	h.Write(bt)
	sum32 := binary.BigEndian.Uint32(h.Sum(nil))
	return (sum32 & samplingBitMask) > sp.ratioMark
}

type taskHolder interface {
	taskRange() (uint64, uint64)
	pick(seq uint64) (bool, uint64)
	revCheck(task uint64) (bool, uint64)
}

type seqTaskHolder struct {
	begin_with uint64
	end_in     uint64
}

func (st seqTaskHolder) taskRange() (uint64, uint64) {
	return st.begin_with, st.end_in
}

func (st seqTaskHolder) pick(seq uint64) (bool, uint64) {
	pickId := seq + st.begin_with
	return pickId <= st.end_in, pickId
}

func (st seqTaskHolder) revCheck(task uint64) (bool, uint64) {
	if task < st.begin_with {
		return false, 0
	}
	return task <= st.end_in, task - st.begin_with
}

type randomTaskHolder struct {
	seqTaskHolder
	taskList   []uint64
	taskLookup map[uint64]uint64
}

func genRandom(beg, end_in uint64) randomTaskHolder {
	rsrc := rand.NewSource(int64(beg))
	rnd := rand.New(rsrc)
	if end_in == indexUnSet {
		panic("must set end index")
	}

	taskList := make([]uint64, end_in-beg+1)
	taskLookup := make(map[uint64]uint64)
	for i := range taskList {
		taskList[i] = beg + uint64(i)
	}

	rnd.Shuffle(len(taskList), func(i, j int) {
		tmp := taskList[j]
		taskList[j] = taskList[i]
		taskList[i] = tmp
	})

	for i, v := range taskList {
		taskLookup[v] = uint64(i)
	}

	return randomTaskHolder{
		seqTaskHolder: seqTaskHolder{
			begin_with: beg,
			end_in:     end_in,
		},
		taskList:   taskList,
		taskLookup: taskLookup,
	}
}

func (rt randomTaskHolder) pick(seq uint64) (bool, uint64) {
	valid, fallback := rt.seqTaskHolder.pick(seq)
	if !valid {
		return valid, fallback
	}
	return true, rt.taskList[seq]
}

func (rt randomTaskHolder) revCheck(task uint64) (bool, uint64) {
	seq, valid := rt.taskLookup[task]
	return valid, seq
}

// task managers cache all task it has assigned
// since the cost is trivial (batch number is limited)
type TaskAssigner struct {
	sync.Mutex
	notifier
	stop_assign bool
	tasks       taskHolder
	progress    uint64
	sampler     *sampling
	runingTasks map[uint64]TaskStatus
}

func construct(start uint64) *TaskAssigner {
	return &TaskAssigner{
		progress:    0,
		runingTasks: make(map[uint64]TaskStatus),
		tasks: seqTaskHolder{
			begin_with: start,
			end_in:     indexUnSet,
		},
	}
}

func (t *TaskAssigner) setEnd(n uint64) *TaskAssigner {
	beg, _ := t.tasks.taskRange()
	t.tasks = seqTaskHolder{
		begin_with: beg,
		end_in:     n,
	}
	return t
}

func (t *TaskAssigner) setMessenger(url string, id int) *TaskAssigner {
	t.notifier = notifier{
		api:            url,
		coordinator_id: id,
	}
	return t
}

func (t *TaskAssigner) setSampling(ratio float32) *TaskAssigner {

	t.sampler = newSampling(ratio) //.setSeed(t.end_in)
	return t
}

func (t *TaskAssigner) setShuffle() *TaskAssigner {

	beg, ed := t.tasks.taskRange()
	t.tasks = genRandom(beg, ed)
	return t
}

func (t *TaskAssigner) stopAssignment(stop bool) {
	t.Lock()
	defer t.Unlock()
	t.stop_assign = stop
}

func (t *TaskAssigner) isStopped() bool {

	t.Lock()
	defer t.Unlock()
	hasTask, _ := t.tasks.pick(t.progress)
	return t.stop_assign && hasTask
}

func (t *TaskAssigner) assign_new() uint64 {

	t.Lock()
	defer t.Unlock()

	target := t.progress
	for tid, status := range t.runingTasks {
		if status == TaskReAssign {
			t.runingTasks[tid] = TaskAssigned
			return tid
		} else if tid >= target {
			target = tid + 1
		}
	}

	upProgress := target == t.progress
	if t.sampler != nil {
		for !t.sampler.sampleInt(int64(target)) {
			if upProgress {
				t.progress++
			} else {
				t.runingTasks[target] = TaskCompleted
			}
			target++
		}
	}

	t.runingTasks[target] = TaskAssigned
	_, pickedId := t.tasks.pick(target)
	return pickedId
}

func (t *TaskAssigner) drop(id uint64) {

	t.Lock()
	defer t.Unlock()

	valid, seq := t.tasks.revCheck(id)
	if !valid {
		log.Printf("invalid id %d\n", id)
	}

	for tid, status := range t.runingTasks {
		if tid == seq {
			if status == TaskAssigned {
				t.runingTasks[tid] = TaskReAssign
			} else {
				log.Printf("unexpected dropping of completed task (%d)\n", id)
			}
			return
		}
	}
	log.Printf("unexpected dropping non-existed task (%d)\n", id)
}

func (t *TaskAssigner) reset(id uint64) {

	t.Lock()
	defer t.Unlock()

	valid, seq := t.tasks.revCheck(id)
	if !valid {
		log.Printf("invalid id %d\n", id)
	}

	t.runingTasks[seq] = TaskReAssign
	log.Printf("enforce reset a task (%d)\n", id)
}

func (t *TaskAssigner) complete(id uint64) (bool, uint64) {
	t.Lock()
	defer t.Unlock()

	valid, seq := t.tasks.revCheck(id)
	if !valid {
		log.Printf("invalid id %d\n", id)
	}

	if _, existed := t.runingTasks[seq]; !existed {
		log.Printf("unexpected completed task (%d)\n", id)
		return false, t.progress
	}
	t.runingTasks[seq] = TaskCompleted

	// scan all tasks and make progress
	completed := []uint64{}
	nowProg := t.progress

	for id, status := range t.runingTasks {
		if status == TaskCompleted {
			completed = append(completed, id)
		}
	}

	sort.Slice(completed, func(i, j int) bool {
		return completed[i] < completed[j]
	})

	for _, id := range completed {
		if id == nowProg {
			delete(t.runingTasks, id)
			nowProg += 1
		} else if id > nowProg {
			break
		} else {
			panic("unexpected prog")
		}
	}

	defer func(newProg uint64) {
		log.Printf("collect completed (%v), now %d, to %d\n", completed, t.progress, newProg)
		t.progress = newProg
	}(nowProg)

	return nowProg > t.progress, nowProg
}

func (t *TaskAssigner) status() (result []uint64, workRange [2]uint64) {

	t.Lock()
	defer t.Unlock()

	workRange[0] = t.progress
	workRange[1] = t.progress

	for id, status := range t.runingTasks {
		if status != TaskCompleted {
			_, tid := t.tasks.pick(id)
			result = append(result, tid)
		}
		if id >= workRange[1] {
			workRange[1] = id
		}
	}

	return
}

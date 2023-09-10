package main

import (
	"fmt"
	"math/rand"
	"strings"
)

func main() {
	var beg, last int64

	fmt.Print("Enter begin block: ")
	_, err := fmt.Scan(&beg)
	if err != nil {
		panic(fmt.Errorf("Error reading integer: %s", err))
	}
	fmt.Print("Enter end block: ")
	_, err = fmt.Scan(&last)
	if err != nil {
		panic(fmt.Errorf("Error reading integer: %s", err))
	}

	fmt.Println("Enter progress/index, separated by comma: ")
	var inputl string
	_, err = fmt.Scanln(&inputl)
	if err != nil {
		panic(fmt.Errorf("Error reading integer lines: %s", err))
	}
	var progs []int64
	for _, istr := range strings.Split(inputl, ",") {
		var prog int64
		if _, err := fmt.Sscanf(istr, "%d", &prog); err == nil {
			progs = append(progs, prog)
		}
	}

	fmt.Println("input ints:", progs)

	indexrev := make(map[int64]string)
	for _, v := range progs {
		indexrev[v] = "no block"
	}

	rsrc := rand.NewSource((beg << 32) + last)
	rnd := rand.New(rsrc)

	taskList := make([]int64, last-beg+1)
	for i := range taskList {
		taskList[i] = beg + int64(i)
	}

	rnd.Shuffle(len(taskList), func(i, j int) {
		tmp := taskList[j]
		taskList[j] = taskList[i]
		taskList[i] = tmp
	})

	for i, v := range taskList {
		if _, existed := indexrev[v]; existed {
			indexrev[v] = fmt.Sprintf("%d", i)
		}
	}

	for _, prog := range progs {
		if prog < int64(len(taskList)) {
			fmt.Printf("Prog %d is index %d, as index it is %s\n", prog, taskList[prog], indexrev[prog])
		} else {
			fmt.Printf("Index %d is %s\n", prog, indexrev[prog])
		}

	}

}
